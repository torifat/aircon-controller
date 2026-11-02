//! Connection to the AirTouch 5 console, turning its pushed state into snapshots.

use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{Context, bail};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::{Notify, mpsc};
use tokio::time::{Instant, interval, sleep, timeout};
use ts_rs::TS;

use crate::models::{
    AcReading, Command, Direction, Fan, SetpointLimits, Snapshot, ZoneControl, ZoneReading,
};
use crate::now;
use crate::protocol::{
    self, AcAbility, AcControl, AcMode, AcPower, AcStatus, Decoder, FanSpeed, Message, Request,
    ZonePower, ZoneStatus, ZoneValue,
};

const PORT: u16 = 9005;
const AC: u8 = 0; // one AC per console here
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY: Duration = Duration::from_secs(10);
// The console only talks when something changes, so poke it after a quiet spell and give
// up on the connection if even that gets no answer.
const KEEPALIVE_AFTER: Duration = Duration::from_secs(4 * 60);
const DEAD_AFTER: Duration = Duration::from_secs(5 * 60);

#[derive(Debug, Clone, Serialize, TS)]
#[ts(export)]
pub struct DeviceInfo {
    pub host: String,
    pub connected: bool,
    pub last_packet_at: Option<f64>,
    pub last_error: Option<String>,
    pub console_version: Option<String>,
}

/// What the controller needs from the console; faked in tests.
pub trait Device: Send + Sync + 'static {
    fn snapshot(&self) -> Option<Snapshot>;
    fn info(&self) -> DeviceInfo;
    /// Notified once fresh state is in after every (re)connect.
    fn ready(&self) -> &Notify;
    fn send_ac(&self, cmd: Command) -> impl Future<Output = anyhow::Result<()>> + Send;
    fn send_zone(
        &self,
        zone: u8,
        on: Option<bool>,
        value: Option<ZoneValue>,
    ) -> impl Future<Output = anyhow::Result<()>> + Send;
}

#[derive(Default)]
struct Shared {
    /// A session is up and has delivered AC and zone status.
    ready: bool,
    last_packet_at: Option<f64>,
    last_error: Option<String>,
    console_version: Option<String>,
    ability: Option<AcAbility>,
    names: BTreeMap<u8, String>,
    ac: Option<AcStatus>,
    zones: BTreeMap<u8, ZoneStatus>,
}

pub struct AirTouch {
    host: String,
    shared: Mutex<Shared>,
    tx: mpsc::Sender<Request>,
    rx: tokio::sync::Mutex<mpsc::Receiver<Request>>,
    ready: Notify,
}

impl AirTouch {
    pub fn new(host: impl Into<String>) -> Arc<Self> {
        let (tx, rx) = mpsc::channel(16);
        Arc::new(Self {
            host: host.into(),
            shared: Mutex::default(),
            tx,
            rx: tokio::sync::Mutex::new(rx),
            ready: Notify::new(),
        })
    }

    /// Keeps a session open forever, reconnecting after failures.
    pub async fn run(self: Arc<Self>) {
        let mut rx = self.rx.lock().await;
        loop {
            // Commands queued while we were down are stale: drop them.
            while rx.try_recv().is_ok() {}
            let err = self.session(&mut rx).await.unwrap_err();
            let message = format!("{err:#}");
            tracing::warn!(
                "AirTouch {}: {message}; retrying in {}s",
                self.host,
                RETRY.as_secs()
            );
            {
                let mut s = self.lock();
                s.ready = false;
                s.last_error = Some(message);
            }
            sleep(RETRY).await;
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(|e| e.into_inner())
    }

    async fn session(&self, rx: &mut mpsc::Receiver<Request>) -> anyhow::Result<()> {
        let stream = timeout(
            CONNECT_TIMEOUT,
            TcpStream::connect((self.host.as_str(), PORT)),
        )
        .await
        .context("connect timed out")?
        .context("connect failed")?;
        let (mut reader, mut writer) = stream.into_split();
        let mut id: u8 = 0;
        let mut packet = |req: &Request| {
            id = id.wrapping_add(1);
            protocol::encode(id, req)
        };

        for req in [
            Request::AcAbility,
            Request::ZoneNames,
            Request::ConsoleVersion,
            Request::ZoneStatus,
            Request::AcStatus,
        ] {
            writer.write_all(&packet(&req)).await?;
        }

        let mut decoder = Decoder::default();
        let mut buf = [0u8; 4096];
        let mut last_rx = Instant::now();
        let mut keepalive_sent = false;
        let mut check = interval(Duration::from_secs(15));
        let mut have = (false, false); // (AC status, zone status) seen this session

        loop {
            tokio::select! {
                n = reader.read(&mut buf) => {
                    let n = n.context("read failed")?;
                    if n == 0 {
                        bail!("connection closed by console");
                    }
                    last_rx = Instant::now();
                    keepalive_sent = false;
                    for msg in decoder.push(&buf[..n]) {
                        match &msg {
                            Message::AcStatus(_) => have.0 = true,
                            Message::ZoneStatus(_) => have.1 = true,
                            _ => {}
                        }
                        self.apply(msg);
                    }
                    let became_ready = {
                        let mut s = self.lock();
                        s.last_packet_at = Some(now());
                        let became = !s.ready && have.0 && have.1;
                        if became {
                            s.ready = true;
                            s.last_error = None;
                        }
                        became
                    };
                    if became_ready {
                        tracing::info!("Connected to AirTouch {}", self.host);
                        self.ready.notify_one();
                    }
                }
                Some(req) = rx.recv() => {
                    writer.write_all(&packet(&req)).await.context("write failed")?;
                }
                _ = check.tick() => {
                    let idle = last_rx.elapsed();
                    if idle > DEAD_AFTER {
                        bail!("no reply for {}s", idle.as_secs());
                    }
                    if idle > KEEPALIVE_AFTER && !keepalive_sent {
                        writer.write_all(&packet(&Request::ConsoleVersion)).await?;
                        keepalive_sent = true;
                    }
                }
            }
        }
    }

    fn apply(&self, msg: Message) {
        let mut s = self.lock();
        match msg {
            Message::AcStatus(acs) => {
                if let Some(ac) = acs.into_iter().find(|a| a.ac == AC) {
                    s.ac = Some(ac);
                }
            }
            Message::ZoneStatus(zones) => {
                for z in zones {
                    s.zones.insert(z.zone, z);
                }
            }
            Message::AcAbility(acs) => s.ability = acs.into_iter().find(|a| a.ac == AC),
            Message::ZoneNames(names) => s.names.extend(names),
            Message::ConsoleVersion { version, .. } => s.console_version = Some(version),
            Message::Other => {}
        }
    }

    async fn send(&self, req: Request) -> anyhow::Result<()> {
        if !self.lock().ready {
            bail!("Not connected to AirTouch");
        }
        self.tx.send(req).await.context("connection task stopped")
    }
}

impl Device for AirTouch {
    fn snapshot(&self) -> Option<Snapshot> {
        let s = self.lock();
        if !s.ready {
            return None;
        }
        let status = s.ac.as_ref()?;
        let ability = s.ability.as_ref();

        let fans = match ability {
            Some(a) => [
                (Fan::Low, a.fan_low),
                (Fan::Medium, a.fan_medium),
                (Fan::High, a.fan_high),
            ]
            .into_iter()
            .filter_map(|(f, ok)| ok.then_some(f))
            .collect(),
            None => vec![Fan::Low, Fan::Medium, Fan::High],
        };
        let limits = ability.map_or_else(SetpointLimits::default, |a| SetpointLimits {
            cool: (f64::from(a.cool_min), f64::from(a.cool_max)),
            heat: (f64::from(a.heat_min), f64::from(a.heat_max)),
        });
        let (mode, direction) = match status.mode {
            AcMode::Auto => ("auto", Direction::None),
            AcMode::Heat => ("heat", Direction::Heat),
            AcMode::Dry => ("dry", Direction::None),
            AcMode::Fan => ("fan", Direction::None),
            AcMode::Cool => ("cool", Direction::Cool),
            AcMode::AutoHeat => ("auto_heat", Direction::Heat),
            AcMode::AutoCool => ("auto_cool", Direction::Cool),
            AcMode::Unknown(_) => ("unknown", Direction::None),
        };
        let ac = AcReading {
            on: status.power.is_on(),
            mode: mode.into(),
            direction,
            fan: match status.fan {
                FanSpeed::Auto => Fan::Auto,
                FanSpeed::Low => Fan::Low,
                FanSpeed::Medium => Fan::Medium,
                FanSpeed::High => Fan::High,
                _ => Fan::Other,
            },
            setpoint: status.setpoint,
            temperature: status.temperature,
            spill: status.spill,
            error_code: status.error_code,
            fans,
            limits,
        };

        let range = ability.map_or(0..u8::MAX, |a| {
            a.start_zone..a.start_zone.saturating_add(a.zone_count)
        });
        let zones = s
            .zones
            .values()
            .filter(|z| range.contains(&z.zone))
            .map(|z| ZoneReading {
                number: z.zone,
                name: s
                    .names
                    .get(&z.zone)
                    .cloned()
                    .unwrap_or_else(|| format!("Zone {}", z.zone + 1)),
                on: z.power != ZonePower::Off,
                control: if z.percentage_control {
                    ZoneControl::Percentage
                } else {
                    ZoneControl::Temperature
                },
                setpoint: z.setpoint,
                temperature: z.temperature.filter(|_| z.has_sensor),
                damper: f64::from(z.open) / 100.0,
                spill: z.spill,
                has_sensor: z.has_sensor,
                low_battery: z.low_battery,
            })
            .collect();
        Some(Snapshot { ac, zones })
    }

    fn info(&self) -> DeviceInfo {
        let s = self.lock();
        DeviceInfo {
            host: self.host.clone(),
            connected: s.ready,
            last_packet_at: s.last_packet_at,
            last_error: s.last_error.clone(),
            console_version: s.console_version.clone(),
        }
    }

    fn ready(&self) -> &Notify {
        &self.ready
    }

    async fn send_ac(&self, cmd: Command) -> anyhow::Result<()> {
        let fan = cmd.fan.and_then(|f| match f {
            Fan::Auto => Some(FanSpeed::Auto),
            Fan::Low => Some(FanSpeed::Low),
            Fan::Medium => Some(FanSpeed::Medium),
            Fan::High => Some(FanSpeed::High),
            Fan::Other => None,
        });
        let control = AcControl {
            ac: AC,
            power: cmd.power,
            fan,
            setpoint: cmd.setpoint,
        };
        self.send(Request::AcControl(vec![control])).await?;
        // As for zones: a check right after must not mistake the console's lag for
        // someone else changing the AC back.
        if let Some(ac) = self.lock().ac.as_mut() {
            if let Some(on) = cmd.power {
                ac.power = if on { AcPower::On } else { AcPower::Off };
            }
            ac.fan = fan.unwrap_or(ac.fan);
            ac.setpoint = cmd.setpoint.or(ac.setpoint);
        }
        self.send(Request::AcStatus).await
    }

    async fn send_zone(
        &self,
        zone: u8,
        on: Option<bool>,
        value: Option<ZoneValue>,
    ) -> anyhow::Result<()> {
        let control = protocol::ZoneControl {
            zone,
            power: on,
            value,
        };
        self.send(Request::ZoneControl(vec![control])).await?;
        // The console takes a second or so to push the zone's new status. Assume the change
        // took until then, so a check right after already sees it (and the push corrects us
        // if it didn't), and ask for that status now rather than waiting.
        {
            let mut s = self.lock();
            if let Some(z) = s.zones.get_mut(&zone) {
                if let Some(on) = on {
                    z.power = if on { ZonePower::On } else { ZonePower::Off };
                }
                // A switch between temperature and percentage control is left for the
                // console to report: the controller waits on it to know the switch took.
                if let Some(ZoneValue::Setpoint(c)) = value {
                    z.setpoint = Some(c);
                }
            }
        }
        self.send(Request::ZoneStatus).await
    }
}
