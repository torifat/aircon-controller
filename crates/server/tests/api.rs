//! End-to-end through the HTTP API, with a fake console that applies commands to itself.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use aircon_server::airtouch::{Device, DeviceInfo};
use aircon_server::api::router;
use aircon_server::controller::Controller;
use aircon_server::models::{
    AcReading, Command, Direction, FAN_LEVELS, Fan, SetpointLimits, Snapshot, ZoneControl,
    ZoneReading,
};
use aircon_server::protocol::ZoneValue;
use aircon_server::store::Store;
use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tokio::sync::Notify;
use tower::ServiceExt;

#[derive(Debug, Clone, PartialEq)]
enum Sent {
    Ac(Command),
    Zone(u8, Option<bool>, Option<ZoneValue>),
}

struct Fake {
    connected: AtomicBool,
    last_error: Mutex<Option<String>>,
    ac: Mutex<AcReading>,
    zones: Mutex<Vec<ZoneReading>>,
    sent: Mutex<Vec<Sent>>,
    ready: Notify,
    /// Zone commands are recorded but not applied, like one the console lost.
    stubborn: AtomicBool,
}

fn zone(number: u8, name: &str, temp: f64, on: bool, control: ZoneControl) -> ZoneReading {
    ZoneReading {
        number,
        name: name.into(),
        on,
        control,
        setpoint: Some(24.0),
        temperature: Some(temp),
        damper: 0.5,
        spill: false,
        has_sensor: true,
        low_battery: false,
    }
}

impl Fake {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            connected: AtomicBool::new(true),
            last_error: Mutex::new(None),
            ac: Mutex::new(AcReading {
                on: true,
                mode: "auto_cool".into(),
                direction: Direction::Cool,
                fan: Fan::Low,
                setpoint: Some(24.0),
                temperature: Some(24.0),
                spill: false,
                error_code: 0,
                fans: FAN_LEVELS.to_vec(),
                limits: SetpointLimits::default(),
            }),
            zones: Mutex::new(vec![
                zone(0, "Master Bed", 24.3, false, ZoneControl::Temperature),
                zone(2, "Bedroom 3", 24.7, true, ZoneControl::Percentage),
                zone(3, "Study", 26.9, true, ZoneControl::Percentage),
            ]),
            sent: Mutex::new(Vec::new()),
            ready: Notify::new(),
            stubborn: AtomicBool::new(false),
        })
    }

    fn sent(&self) -> Vec<Sent> {
        self.sent.lock().unwrap().clone()
    }

    fn sent_ac(&self) -> Vec<Sent> {
        let mut sent = self.sent();
        sent.retain(|s| matches!(s, Sent::Ac(_)));
        sent
    }

    fn set_connected(&self, on: bool) {
        self.connected.store(on, Ordering::SeqCst);
    }
}

impl Device for Fake {
    fn snapshot(&self) -> Option<Snapshot> {
        self.connected.load(Ordering::SeqCst).then(|| Snapshot {
            ac: self.ac.lock().unwrap().clone(),
            zones: self.zones.lock().unwrap().clone(),
        })
    }

    fn info(&self) -> DeviceInfo {
        DeviceInfo {
            host: "192.0.2.1".into(),
            connected: self.connected.load(Ordering::SeqCst),
            last_packet_at: None,
            last_error: self.last_error.lock().unwrap().clone(),
            console_version: Some("1.3.3".into()),
        }
    }

    fn ready(&self) -> &Notify {
        &self.ready
    }

    async fn send_ac(&self, cmd: Command) -> anyhow::Result<()> {
        self.sent.lock().unwrap().push(Sent::Ac(cmd));
        let mut ac = self.ac.lock().unwrap();
        ac.on = cmd.power.unwrap_or(ac.on);
        ac.fan = cmd.fan.unwrap_or(ac.fan);
        ac.setpoint = cmd.setpoint.or(ac.setpoint);
        Ok(())
    }

    async fn send_zone(
        &self,
        number: u8,
        on: Option<bool>,
        value: Option<ZoneValue>,
    ) -> anyhow::Result<()> {
        self.sent
            .lock()
            .unwrap()
            .push(Sent::Zone(number, on, value));
        for z in self
            .zones
            .lock()
            .unwrap()
            .iter_mut()
            .filter(|z| z.number == number)
        {
            z.on = on.unwrap_or(z.on);
            if !self.stubborn.load(Ordering::SeqCst) {
                match value {
                    Some(ZoneValue::Setpoint(c)) => {
                        z.control = ZoneControl::Temperature;
                        z.setpoint = Some(c);
                    }
                    Some(ZoneValue::Open(p)) => {
                        z.control = ZoneControl::Percentage;
                        z.damper = f64::from(p) / 100.0;
                    }
                    None => {}
                }
            }
        }
        Ok(())
    }
}

struct Env {
    app: Router,
    fake: Arc<Fake>,
    clock: Arc<AtomicU64>,
    controller: Arc<Controller<Fake>>,
}

const START: f64 = 1_000_000.0;

impl Env {
    fn new() -> Self {
        Self::with_store(Fake::new(), Store::memory().unwrap())
    }

    fn with_store(fake: Arc<Fake>, store: Store) -> Self {
        let clock = Arc::new(AtomicU64::new(START.to_bits()));
        let c = Arc::clone(&clock);
        let controller = Controller::new(
            Arc::clone(&fake),
            store,
            Arc::new(move || f64::from_bits(c.load(Ordering::SeqCst))),
        );
        Self {
            app: router(Arc::clone(&controller), None),
            fake,
            clock,
            controller,
        }
    }

    fn now(&self) -> f64 {
        f64::from_bits(self.clock.load(Ordering::SeqCst))
    }

    fn advance(&self, secs: f64) {
        self.clock
            .store((self.now() + secs).to_bits(), Ordering::SeqCst);
    }

    async fn call(&self, method: Method, uri: &str, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(uri);
        let body = match body {
            Some(b) => {
                req = req.header("content-type", "application/json");
                Body::from(b.to_string())
            }
            None => Body::empty(),
        };
        let res = self
            .app
            .clone()
            .oneshot(req.body(body).unwrap())
            .await
            .unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    async fn get(&self, uri: &str) -> Value {
        let (status, body) = self.call(Method::GET, uri, None).await;
        assert_eq!(status, StatusCode::OK, "{uri}: {body}");
        body
    }

    async fn tick(&self) -> Value {
        self.call(Method::POST, "/api/tick", None).await.1
    }

    async fn go_live(&self) {
        let (status, _) = self
            .call(
                Method::PATCH,
                "/api/settings",
                Some(json!({ "dry_run": false })),
            )
            .await;
        assert_eq!(status, StatusCode::OK);
    }

    async fn titles(&self, filter: &[&str]) -> Vec<String> {
        let items = self.get("/api/activity").await["items"]
            .as_array()
            .unwrap()
            .clone();
        items
            .iter()
            .filter(|i| filter.is_empty() || filter.contains(&i["kind"].as_str().unwrap()))
            .map(|i| i["title"].as_str().unwrap().to_owned())
            .collect()
    }
}

fn zone_json<'a>(state: &'a Value, name: &str) -> &'a Value {
    state["zones"]
        .as_array()
        .unwrap()
        .iter()
        .find(|z| z["name"] == name)
        .unwrap()
}

#[tokio::test]
async fn dry_run_by_default_sends_nothing() {
    let env = Env::new();
    let state = env.tick().await;
    assert_eq!(state["controller"]["status"], "running");
    assert_eq!(state["controller"]["dry_run"], true);
    assert_eq!(
        state["controller"]["command"]["text"],
        "fan high, setpoint 21°"
    );
    assert!(env.fake.sent().is_empty());
    let titles = env.titles(&[]).await;
    assert!(
        titles.contains(&"Would: Fan Low → High · AC setpoint 24.0° → 21.0°".to_owned()),
        "{titles:?}"
    );
}

#[tokio::test]
async fn live_run_sends_and_reports() {
    let env = Env::new();
    env.go_live().await;
    let state = env.tick().await;
    let want = Command {
        power: None,
        fan: Some(Fan::High),
        setpoint: Some(21.0),
    };
    assert_eq!(env.fake.sent_ac(), vec![Sent::Ac(want)]);
    let study = zone_json(&state, "Study");
    assert_eq!(study["driving"], true);
    assert_eq!(study["error"], 2.9);
    assert_eq!(state["ac"]["limits"]["cool"], json!([18.0, 25.0]));
}

#[tokio::test]
async fn manual_change_pauses() {
    let env = Env::new();
    env.go_live().await;
    env.tick().await;
    env.fake.ac.lock().unwrap().fan = Fan::Medium; // someone used the AirTouch app
    env.advance(60.0);
    let state = env.tick().await;
    assert_eq!(state["controller"]["status"], "paused");
    assert_eq!(state["controller"]["paused_until"], env.now() + 1800.0);
    assert!(!env.titles(&["external"]).await.is_empty());

    env.call(Method::POST, "/api/resume", None).await;
    env.advance(60.0);
    assert_eq!(env.tick().await["controller"]["status"], "running");
}

#[tokio::test]
async fn percentage_zone_target_is_kept_locally() {
    let env = Env::new();
    env.tick().await;
    let (status, _) = env
        .call(
            Method::PATCH,
            "/api/zones/3",
            Some(json!({ "target": 26.5 })),
        )
        .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        env.fake.sent().is_empty(),
        "AirTouch ignores setpoints for % zones"
    );
    assert_eq!(
        env.get("/api/settings").await["targets"],
        json!({ "3": 26.5 })
    );
    let state = env.tick().await;
    assert_eq!(zone_json(&state, "Study")["target"], 26.5);
}

#[tokio::test]
async fn temperature_zone_target_goes_to_airtouch() {
    let env = Env::new();
    env.tick().await;
    env.call(
        Method::PATCH,
        "/api/zones/0",
        Some(json!({ "on": true, "target": 23.5 })),
    )
    .await;
    assert_eq!(
        env.fake.sent(),
        vec![Sent::Zone(0, Some(true), Some(ZoneValue::Setpoint(23.5)))]
    );
    assert!(
        env.titles(&[])
            .await
            .contains(&"Master Bed turned on".to_owned())
    );
}

#[tokio::test]
async fn zone_changes_from_app_are_logged() {
    let env = Env::new();
    env.tick().await;
    env.fake.zones.lock().unwrap()[1].on = false;
    env.advance(60.0);
    env.tick().await;
    assert_eq!(env.titles(&["external"]).await[0], "Bedroom 3 turned off");
}

#[tokio::test]
async fn unknown_zone_is_404() {
    let env = Env::new();
    env.tick().await;
    let (status, _) = env
        .call(Method::PATCH, "/api/zones/9", Some(json!({ "on": true })))
        .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn settings_validation() {
    let env = Env::new();
    for bad in [
        json!({ "deadband": 10 }),
        json!({ "fan_medium_at": 3 }),
        json!({ "bogus": 1 }),
    ] {
        let (status, _) = env
            .call(Method::PATCH, "/api/settings", Some(bad.clone()))
            .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
    }
    let (_, body) = env
        .call(
            Method::PATCH,
            "/api/settings",
            Some(json!({ "zones": [3, 2, 3] })),
        )
        .await;
    assert_eq!(body["zones"], json!([2, 3]));
    let (_, body) = env
        .call(
            Method::PATCH,
            "/api/settings",
            Some(json!({ "deadband": 1.5 })),
        )
        .await;
    assert_eq!(body["zones"], json!([2, 3]), "untouched when absent");
    let (_, body) = env
        .call(
            Method::PATCH,
            "/api/settings",
            Some(json!({ "zones": null })),
        )
        .await;
    assert_eq!(body["zones"], Value::Null);
}

#[tokio::test]
async fn disconnected_and_starting() {
    let env = Env::new();
    env.fake.set_connected(false);
    let state = env.tick().await;
    assert_eq!(state["controller"]["status"], "starting");
    assert_eq!(state["controller"]["dry_run"], true);
    let (status, _) = env.call(Method::GET, "/readyz", None).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);

    env.fake.set_connected(true);
    env.tick().await;
    env.fake.set_connected(false);
    assert_eq!(env.tick().await["controller"]["status"], "disconnected");
}

#[tokio::test]
async fn first_connection_failure_is_an_error() {
    let env = Env::new();
    env.fake.set_connected(false);
    *env.fake.last_error.lock().unwrap() = Some("timed out".into());
    let state = env.tick().await;
    assert_eq!(state["controller"]["status"], "disconnected");
    assert_eq!(
        state["controller"]["reason"],
        "Can't reach AirTouch (timed out)"
    );
}

#[tokio::test]
async fn connection_log_skips_startup() {
    let env = Env::new();
    env.fake.set_connected(false);
    env.tick().await;
    env.fake.set_connected(true);
    env.tick().await;
    assert!(env.titles(&["info", "error"]).await.is_empty());

    env.fake.set_connected(false);
    env.tick().await;
    env.fake.set_connected(true);
    env.tick().await;
    let titles = env.titles(&["info", "error"]).await;
    assert_eq!(titles[0], "Reconnected to AirTouch");
    assert!(titles[1].starts_with("Can't reach AirTouch"), "{titles:?}");
}

#[tokio::test]
async fn history() {
    let env = Env::new();
    for _ in 0..3 {
        env.tick().await;
        env.advance(60.0);
    }
    let h = env.get("/api/history?hours=1").await;
    assert_eq!(h["ac"].as_array().unwrap().len(), 3);
    let study = h["zones"]
        .as_array()
        .unwrap()
        .iter()
        .find(|z| z["name"] == "Study")
        .unwrap();
    assert_eq!(study["points"][0]["temperature"], 26.9);
    assert_eq!(study["points"][0]["target"], 24.0);
}

#[tokio::test]
async fn ac_power_from_ui_hands_control_back() {
    let env = Env::new();
    env.go_live().await;
    env.tick().await;
    let (status, _) = env
        .call(Method::PUT, "/api/ac", Some(json!({ "on": false })))
        .await;
    assert_eq!(status, StatusCode::OK);
    env.advance(3600.0);
    let state = env.tick().await;
    assert_eq!(state["controller"]["status"], "off");
    assert_eq!(
        env.fake.sent_ac().last(),
        Some(&Sent::Ac(Command {
            power: Some(false),
            ..Command::default()
        }))
    );
    assert!(
        !env.titles(&["external"])
            .await
            .iter()
            .any(|t| t.starts_with("AC")),
        "our own off isn't a manual change"
    );
}

#[tokio::test]
async fn pause_from_ui() {
    let env = Env::new();
    let (_, body) = env
        .call(Method::POST, "/api/pause", Some(json!({ "minutes": 15 })))
        .await;
    assert_eq!(body["paused_until"], START + 900.0);
    assert_eq!(env.tick().await["controller"]["status"], "paused");
}

#[tokio::test]
async fn settings_survive_restart() {
    let dir = std::env::temp_dir().join(format!("aircon-test-{}", std::process::id()));
    let path = dir.join("aircon.db");
    {
        let env = Env::with_store(Fake::new(), Store::open(&path).unwrap());
        env.call(
            Method::PATCH,
            "/api/settings",
            Some(json!({ "deadband": 1.5 })),
        )
        .await;
    }
    let env = Env::with_store(Fake::new(), Store::open(&path).unwrap());
    assert_eq!(env.controller.state().await.settings.deadband, 1.5);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn quick_target_changes_each_start_from_the_last() {
    let env = Env::new();
    env.tick().await;
    // Two taps before the next check: the second must start from the first, not from
    // what the last check saw.
    for target in [23.5, 23.0] {
        let body = json!({ "target": target });
        let (status, _) = env.call(Method::PATCH, "/api/zones/0", Some(body)).await;
        assert_eq!(status, StatusCode::OK);
    }
    let titles = env.titles(&["info"]).await;
    assert_eq!(
        titles[..2],
        [
            "Master Bed target 23.5° → 23.0°",
            "Master Bed target 24.0° → 23.5°"
        ]
    );
}

#[tokio::test]
async fn dry_run_logs_a_repeated_decision_once() {
    let env = Env::new();
    for _ in 0..3 {
        env.tick().await;
        env.advance(60.0);
    }
    let would = async || {
        env.titles(&["decision"])
            .await
            .into_iter()
            .filter(|t| t.starts_with("Would: Fan") || t.starts_with("Would: AC"))
            .collect::<Vec<_>>()
    };
    assert_eq!(would().await.len(), 1, "{:?}", would().await);
    let dampers = env.titles(&["decision"]).await.len() - 1;
    assert_eq!(dampers, 1, "damper moves are logged once too");

    // A different decision is logged again.
    env.fake.zones.lock().unwrap()[2].temperature = Some(25.5);
    env.tick().await;
    assert_eq!(would().await.len(), 2);
}

#[tokio::test]
async fn state_reports_the_build() {
    let env = Env::new();
    let state = env.get("/api/state").await;
    assert_eq!(state["build"]["version"], "dev");
}

#[tokio::test]
async fn controller_runs_the_dampers() {
    let env = Env::new();
    env.go_live().await;
    env.tick().await;
    // Master Bed (off) switches to percentage control, Study needs it most, and Bedroom 3
    // is 0.7° over.
    let opened: Vec<_> = env
        .fake
        .sent()
        .into_iter()
        .filter(|s| matches!(s, Sent::Zone(..)))
        .collect();
    assert_eq!(
        opened,
        vec![
            Sent::Zone(0, None, Some(ZoneValue::Open(100))),
            Sent::Zone(2, None, Some(ZoneValue::Open(75))),
            Sent::Zone(3, None, Some(ZoneValue::Open(100))),
        ]
    );
    assert_eq!(
        env.titles(&["decision"]).await[0],
        "Master Bed to percentage control at 100% · Bedroom 3 50% → 75% · Study 50% → 100%"
    );
    assert_eq!(env.get("/api/settings").await["targets"]["0"], 24.0);
    env.advance(60.0);
    env.tick().await;
    assert!(env.titles(&["external"]).await.is_empty(), "our own switch");

    // Bedroom 3 reaches target and closes down; nothing else moves.
    let before = env.fake.sent().len();
    env.fake.zones.lock().unwrap()[1].temperature = Some(23.9);
    env.advance(60.0);
    env.tick().await;
    assert_eq!(
        env.fake.sent()[before..],
        [Sent::Zone(2, None, Some(ZoneValue::Open(10)))]
    );

    // Paused: hands off the dampers too.
    env.call(Method::POST, "/api/pause", Some(json!({ "minutes": 15 })))
        .await;
    env.fake.zones.lock().unwrap()[1].temperature = Some(25.5);
    let before = env.fake.sent().len();
    env.advance(60.0);
    env.tick().await;
    assert_eq!(env.fake.sent().len(), before);
}

#[tokio::test]
async fn dampers_rest_while_the_ac_is_off() {
    let env = Env::new();
    {
        let mut zones = env.fake.zones.lock().unwrap();
        zones[1].temperature = Some(23.9);
        zones[2].temperature = Some(24.0);
    }
    env.go_live().await;
    let zone_sends = || {
        env.fake
            .sent()
            .into_iter()
            .filter(|s| matches!(s, Sent::Zone(..)))
            .count()
    };
    // Every room at target parks the AC, and the dampers stay where they are.
    env.tick().await;
    assert_eq!(
        env.fake.sent_ac()[0],
        Sent::Ac(Command {
            power: Some(false),
            fan: None,
            setpoint: None
        })
    );
    assert_eq!(zone_sends(), 0);
    // Rooms swapping which is warmest while parked doesn't move them either.
    env.fake.zones.lock().unwrap()[1].temperature = Some(24.1);
    env.advance(60.0);
    env.tick().await;
    assert_eq!(zone_sends(), 0);

    // Study drifts past the deadband: the check that turns the AC on sets the dampers.
    env.fake.zones.lock().unwrap()[2].temperature = Some(26.5);
    env.advance(360.0);
    env.tick().await;
    assert!(env.fake.ac.lock().unwrap().on);
    assert!(zone_sends() > 0);
}
