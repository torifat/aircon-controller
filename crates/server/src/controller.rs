//! Runs the decision loop against the console and records what happened.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{Mutex, Notify};

use crate::airtouch::Device;
use crate::logic::{Damper, damper, decide};
use crate::models::{
    ActivityKind, Command, Decision, Runtime, Settings, Snapshot, Status, ZoneControl, ZoneReading,
};
use crate::protocol::ZoneValue;
use crate::store::Store;

const RETENTION_DAYS: f64 = 30.0;
/// A zone change seen within this long of one sent from the UI is ours, not the app's.
const INTENT_WINDOW: f64 = 120.0;

pub type Clock = Arc<dyn Fn() -> f64 + Send + Sync>;

pub struct Controller<D> {
    pub device: Arc<D>,
    pub store: Store,
    pub clock: Clock,
    state: Mutex<State>,
    wake: Notify,
}

pub struct State {
    pub settings: Settings,
    pub runtime: Runtime,
    pub decision: Option<Decision>,
    pub snapshot: Option<Snapshot>,
    pub next_tick_at: Option<f64>,
    last_status: Option<Status>,
    zone_intents: HashMap<u8, f64>,
    last_prune: f64,
    /// The last dry-run decision logged. Nothing changes on the AC in dry run, so the same
    /// decision comes up every tick; it's only logged again once it differs.
    last_would: Option<String>,
    /// The same for a room the controller would open in dry run.
    last_would_open: Option<u8>,
}

#[derive(Debug)]
pub enum ActionError {
    Offline(String),
    NoZone(u8),
}

impl<D: Device> Controller<D> {
    pub fn new(device: Arc<D>, store: Store, clock: Clock) -> Arc<Self> {
        let state = State {
            settings: store.load_settings(),
            runtime: store.load_runtime(),
            decision: None,
            snapshot: None,
            next_tick_at: None,
            last_status: None,
            zone_intents: HashMap::new(),
            last_prune: 0.0,
            last_would: None,
            last_would_open: None,
        };
        Arc::new(Self {
            device,
            store,
            clock,
            state: Mutex::new(state),
            wake: Notify::new(),
        })
    }

    pub async fn state(&self) -> tokio::sync::MutexGuard<'_, State> {
        self.state.lock().await
    }

    /// Ticks forever: every `tick_seconds`, on `poke`, and whenever the console (re)connects.
    pub async fn run(self: Arc<Self>) {
        loop {
            self.tick().await;
            let wait = {
                let mut st = self.state.lock().await;
                st.next_tick_at = Some((self.clock)() + st.settings.tick_seconds);
                Duration::from_secs_f64(st.settings.tick_seconds)
            };
            tokio::select! {
                () = tokio::time::sleep(wait) => {}
                () = self.wake.notified() => {}
                () = self.device.ready().notified() => {}
            }
        }
    }

    /// Run the next tick now, e.g. after a change from the UI.
    pub fn poke(&self) {
        self.wake.notify_one();
    }

    pub async fn tick(&self) -> Decision {
        let mut st = self.state.lock().await;
        let now = (self.clock)();

        let Some(snap) = self.device.snapshot() else {
            let error = self.device.info().last_error;
            let decision = match error {
                None if st.snapshot.is_none() => {
                    Decision::bare(Status::Starting, "Connecting to AirTouch")
                }
                error => Decision::bare(
                    Status::Disconnected,
                    format!(
                        "Can't reach AirTouch ({})",
                        error.as_deref().unwrap_or("no data")
                    ),
                ),
            };
            self.note_status(&mut st, &decision, now);
            st.decision = Some(decision.clone());
            return decision;
        };

        if let Some(before) = &st.snapshot {
            self.log_zone_changes(&st, before, &snap, now);
        }

        let previous = st.runtime.clone();
        let (decision, mut runtime) = decide(&snap, &st.settings, &previous, now);

        for e in &decision.events {
            self.store.log(now, e.kind, &e.title, &e.reason, false);
        }

        if let Some(cmd) = decision.command {
            let title = describe(&cmd, &snap);
            if decision.dry_run {
                let would = format!("Would: {title}");
                if st.last_would.as_deref() != Some(would.as_str()) {
                    self.store
                        .log(now, ActivityKind::Decision, &would, &decision.reason, true);
                    st.last_would = Some(would);
                }
            } else if let Err(e) = self.device.send_ac(cmd).await {
                tracing::warn!("sending {cmd:?} failed: {e:#}");
                self.store.log(
                    now,
                    ActivityKind::Error,
                    &format!("Couldn't send: {title}"),
                    &format!("{e:#}"),
                    false,
                );
                // Nothing changed on the AC, so don't expect it to have.
                runtime.expected = None;
                runtime.parked = previous.parked;
                runtime.last_power_change = previous.last_power_change;
            } else {
                self.store
                    .log(now, ActivityKind::Decision, &title, &decision.reason, false);
            }
        }

        if decision.command.is_none() || !decision.dry_run {
            st.last_would = None;
        }
        self.move_damper(&mut st, &mut runtime, &snap, now).await;
        self.note_status(&mut st, &decision, now);
        if let Err(e) = self.store.record(now, &snap, &decision) {
            tracing::error!("recording readings failed: {e:#}");
        }
        self.store.save_runtime(&runtime);
        st.runtime = runtime;
        st.snapshot = Some(snap);
        st.decision = Some(decision.clone());

        if now - st.last_prune > 3600.0 {
            if let Err(e) = self.store.prune(now - RETENTION_DAYS * 86400.0) {
                tracing::error!("pruning failed: {e:#}");
            }
            st.last_prune = now;
        }
        decision
    }

    /// Carry out the lone-room rule. In dry run, opening a room is only logged, once.
    async fn move_damper(&self, st: &mut State, runtime: &mut Runtime, snap: &Snapshot, now: f64) {
        let change = damper(snap, &st.settings, runtime, now);
        let would = if st.settings.enabled && st.settings.dry_run {
            let s = Settings {
                dry_run: false,
                ..st.settings.clone()
            };
            match damper(snap, &s, runtime, now) {
                Some(Damper::Open { zone, name, target }) => Some((zone, name, target)),
                _ => None,
            }
        } else {
            None
        };
        match would {
            Some((zone, name, target)) if st.last_would_open != Some(zone) => {
                self.store.log(
                    now,
                    ActivityKind::Decision,
                    &format!("Would: {name} fully open"),
                    &open_reason(target),
                    true,
                );
                st.last_would_open = Some(zone);
            }
            Some(_) => {}
            None => st.last_would_open = None,
        }

        let Some(change) = change else { return };
        let (zone, value, title, reason) = match &change {
            Damper::Release => {
                runtime.opened = None;
                return;
            }
            Damper::Open { zone, name, target } => (
                *zone,
                ZoneValue::Open(100),
                format!("{name} fully open"),
                open_reason(*target),
            ),
            Damper::Restore {
                zone,
                name,
                target,
                reason,
            } => (
                *zone,
                ZoneValue::Setpoint(*target),
                format!("{name} back to temperature control at {target:.1}°"),
                reason.clone(),
            ),
        };
        st.zone_intents.insert(zone, now);
        if let Err(e) = self.device.send_zone(zone, None, Some(value)).await {
            tracing::warn!("sending {change:?} failed: {e:#}");
            self.store.log(
                now,
                ActivityKind::Error,
                &format!("Couldn't send: {title}"),
                &format!("{e:#}"),
                false,
            );
            return;
        }
        match change {
            Damper::Open { zone, target, .. } => {
                st.settings.targets.insert(zone, target);
                runtime.opened = Some(zone);
            }
            _ => {
                st.settings.targets.remove(&zone);
                runtime.opened = None;
            }
        }
        self.store.save_settings(&st.settings);
        self.store
            .log(now, ActivityKind::Decision, &title, &reason, false);
        // Check again with the zone as it is now.
        self.poke();
    }

    fn note_status(&self, st: &mut State, decision: &Decision, now: f64) {
        if st.last_status == Some(decision.status) {
            return;
        }
        match decision.status {
            Status::Disconnected | Status::Error => {
                self.store
                    .log(now, ActivityKind::Error, &decision.reason, "", false);
            }
            _ if st.last_status == Some(Status::Disconnected) => {
                self.store.log(
                    now,
                    ActivityKind::Info,
                    "Reconnected to AirTouch",
                    "",
                    false,
                );
            }
            _ => {}
        }
        st.last_status = Some(decision.status);
    }

    fn log_zone_changes(&self, st: &State, before: &Snapshot, after: &Snapshot, now: f64) {
        for z in &after.zones {
            let Some(prev) = before.zones.iter().find(|p| p.number == z.number) else {
                continue;
            };
            if st
                .zone_intents
                .get(&z.number)
                .is_some_and(|&t| t > now - INTENT_WINDOW)
            {
                continue;
            }
            for title in zone_diff(prev, z) {
                self.store.log(
                    now,
                    ActivityKind::External,
                    &title,
                    "Changed in the AirTouch app",
                    false,
                );
            }
        }
    }

    // actions from the API

    pub async fn set_zone(
        &self,
        number: u8,
        on: Option<bool>,
        target: Option<f64>,
    ) -> Result<(), ActionError> {
        let mut st = self.state.lock().await;
        let zone = self.zone(&st, number)?;
        let now = (self.clock)();
        st.zone_intents.insert(number, now);

        let mut titles = Vec::new();
        if let Some(on) = on.filter(|&on| on != zone.on) {
            titles.push(format!(
                "{} turned {}",
                zone.name,
                if on { "on" } else { "off" }
            ));
        }
        let mut setpoint = target;
        if let Some(t) = target {
            let current = st.settings.targets.get(&number).copied().or(zone.setpoint);
            titles.push(format!("{} target {} → {t:.1}°", zone.name, deg(current)));
            if zone.control == ZoneControl::Percentage {
                // The AirTouch ignores setpoints in percentage control, so we keep our own.
                st.settings.targets.insert(number, t);
                self.store.save_settings(&st.settings);
                setpoint = None;
            }
        }
        if on.is_some() || setpoint.is_some() {
            self.device
                .send_zone(number, on, setpoint.map(ZoneValue::Setpoint))
                .await
                .map_err(|e| ActionError::Offline(format!("{e:#}")))?;
        }
        for title in titles {
            self.store
                .log(now, ActivityKind::Info, &title, "From Climate", false);
        }
        drop(st);
        self.poke();
        Ok(())
    }

    pub async fn set_ac_power(&self, on: bool) -> Result<(), ActionError> {
        let mut st = self.state.lock().await;
        let now = (self.clock)();
        self.device
            .send_ac(Command {
                power: Some(on),
                ..Command::default()
            })
            .await
            .map_err(|e| ActionError::Offline(format!("{e:#}")))?;
        let rt = &mut st.runtime;
        rt.parked = false;
        rt.last_power_change = Some(now);
        if let Some(exp) = rt.expected.as_mut() {
            exp.power = on;
        }
        self.store.save_runtime(&st.runtime);
        self.store.log(
            now,
            ActivityKind::Info,
            if on { "AC on" } else { "AC off" },
            "From Climate",
            false,
        );
        drop(st);
        self.poke();
        Ok(())
    }

    pub async fn pause(&self, minutes: f64) -> f64 {
        let mut st = self.state.lock().await;
        let now = (self.clock)();
        let until = st.runtime.paused_until.unwrap_or(now).max(now) + minutes * 60.0;
        st.runtime.paused_until = Some(until);
        st.runtime.pause_reason = Some("Paused from Climate".into());
        self.store.save_runtime(&st.runtime);
        self.store.log(
            now,
            ActivityKind::Info,
            &format!("Paused for {minutes} min"),
            "From Climate",
            false,
        );
        drop(st);
        self.poke();
        until
    }

    pub async fn resume(&self) {
        let mut st = self.state.lock().await;
        let rt = &mut st.runtime;
        rt.paused_until = None;
        rt.pause_reason = None;
        rt.expected = None;
        self.store.save_runtime(&st.runtime);
        self.store.log(
            (self.clock)(),
            ActivityKind::Info,
            "Controller resumed",
            "From Climate",
            false,
        );
        drop(st);
        self.poke();
    }

    pub async fn update_settings(&self, after: Settings) -> Settings {
        let mut st = self.state.lock().await;
        let before = std::mem::replace(&mut st.settings, after.clone());
        self.store.save_settings(&after);
        let changes = settings_diff(&before, &after);
        if !changes.is_empty() {
            self.store.log(
                (self.clock)(),
                ActivityKind::Info,
                "Settings changed",
                &changes.join(", "),
                false,
            );
            if before.enabled != after.enabled || before.dry_run != after.dry_run {
                // Start watching for manual changes afresh.
                st.runtime.expected = None;
                st.runtime.parked = false;
                self.store.save_runtime(&st.runtime);
            }
            drop(st);
            self.poke();
        }
        after
    }

    /// The zone as the console has it now, not as of the last check: changes in quick
    /// succession must each start from the one before.
    fn zone(&self, st: &State, number: u8) -> Result<ZoneReading, ActionError> {
        let snap = self
            .device
            .snapshot()
            .or_else(|| st.snapshot.clone())
            .ok_or_else(|| ActionError::Offline("Not connected to AirTouch".into()))?;
        snap.zones
            .into_iter()
            .find(|z| z.number == number)
            .ok_or(ActionError::NoZone(number))
    }
}

fn open_reason(target: f64) -> String {
    format!("Only room on; the AC holds it at {target:.1}° instead of spilling")
}

fn deg(v: Option<f64>) -> String {
    v.map_or_else(|| "—".into(), |v| format!("{v:.1}°"))
}

/// Activity title for a command, e.g. "Fan Low → High · AC setpoint 24.0° → 21.0°".
pub fn describe(cmd: &Command, snap: &Snapshot) -> String {
    let ac = &snap.ac;
    let resuming = cmd.power == Some(true);
    let mut parts = Vec::new();
    if let Some(on) = cmd.power {
        parts.push(if on {
            "AC on".to_owned()
        } else {
            "AC off".to_owned()
        });
    }
    if let Some(fan) = cmd.fan {
        parts.push(if resuming {
            format!("fan {}", fan.title())
        } else {
            format!("Fan {} → {}", ac.fan.title(), fan.title())
        });
    }
    if let Some(sp) = cmd.setpoint {
        parts.push(if resuming {
            format!("setpoint {sp:.1}°")
        } else {
            format!("AC setpoint {} → {sp:.1}°", deg(ac.setpoint))
        });
    }
    parts.join(" · ")
}

/// Short text for the API, e.g. "fan high, setpoint 21°".
pub fn summary(cmd: &Command) -> String {
    let mut parts = Vec::new();
    if let Some(on) = cmd.power {
        parts.push(if on {
            "AC on".to_owned()
        } else {
            "AC off".to_owned()
        });
    }
    if let Some(fan) = cmd.fan {
        parts.push(format!("fan {}", fan.title().to_lowercase()));
    }
    if let Some(sp) = cmd.setpoint {
        parts.push(format!("setpoint {sp}°"));
    }
    if parts.is_empty() {
        "no change".into()
    } else {
        parts.join(", ")
    }
}

fn zone_diff(before: &ZoneReading, after: &ZoneReading) -> Vec<String> {
    let mut titles = Vec::new();
    if before.on != after.on {
        titles.push(format!(
            "{} turned {}",
            after.name,
            if after.on { "on" } else { "off" }
        ));
    }
    if before.setpoint != after.setpoint && after.control == ZoneControl::Temperature {
        titles.push(format!(
            "{} target {} → {}",
            after.name,
            deg(before.setpoint),
            deg(after.setpoint)
        ));
    }
    if before.control != after.control {
        let control = if after.control == ZoneControl::Percentage {
            "percentage"
        } else {
            "temperature"
        };
        titles.push(format!("{} switched to {control} control", after.name));
    }
    titles
}

fn settings_diff(before: &Settings, after: &Settings) -> Vec<String> {
    let (Ok(serde_json::Value::Object(a)), Ok(serde_json::Value::Object(b))) =
        (serde_json::to_value(before), serde_json::to_value(after))
    else {
        return Vec::new();
    };
    a.iter()
        .filter_map(|(k, va)| {
            let vb = b.get(k)?;
            (va != vb).then(|| format!("{k} {} → {}", show(va), show(vb)))
        })
        .collect()
}

fn show(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Bool(true) => "on".into(),
        serde_json::Value::Bool(false) => "off".into(),
        serde_json::Value::Null => "all".into(),
        other => other.to_string(),
    }
}
