//! HTTP API for the UI. Response and request shapes are exported to TypeScript.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, patch, post, put};
use axum::{Json, Router};
use serde::{Deserialize, Deserializer, Serialize};
use tower_http::services::{ServeDir, ServeFile};
use ts_rs::TS;

use crate::airtouch::{Device, DeviceInfo};
use crate::controller::{ActionError, Controller, summary};
use crate::models::{
    ActivityKind, Command, Decision, Direction, Fan, Settings, Status, ZoneEval, ZoneReading,
};
use crate::store::{AC_ROW, ActivityItem};

type Ctl<D> = State<Arc<Controller<D>>>;

pub fn router<D: Device>(controller: Arc<Controller<D>>, ui_dir: Option<PathBuf>) -> Router {
    let api = Router::new()
        .route(
            "/healthz",
            get(|| async { Json(serde_json::json!({ "ok": true })) }),
        )
        .route("/readyz", get(readyz::<D>))
        .route("/api/state", get(state::<D>))
        .route("/api/tick", post(tick::<D>))
        .route("/api/history", get(history::<D>))
        .route("/api/activity", get(activity::<D>))
        .route(
            "/api/settings",
            get(get_settings::<D>).patch(patch_settings::<D>),
        )
        .route("/api/zones/{number}", patch(patch_zone::<D>))
        .route("/api/ac", put(put_ac::<D>))
        .route("/api/pause", post(pause::<D>))
        .route("/api/resume", post(resume::<D>))
        .with_state(controller);
    match ui_dir {
        // Single-page app: unknown paths fall back to index.html.
        Some(dir) => {
            let index = dir.join("index.html");
            api.fallback_service(ServeDir::new(dir).fallback(ServeFile::new(index)))
        }
        None => api,
    }
}

// errors

pub enum ApiError {
    Unavailable(String),
    NotFound(String),
    Invalid(String),
    Internal(anyhow::Error),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, detail) = match self {
            Self::Unavailable(m) => (StatusCode::SERVICE_UNAVAILABLE, m),
            Self::NotFound(m) => (StatusCode::NOT_FOUND, m),
            Self::Invalid(m) => (StatusCode::UNPROCESSABLE_ENTITY, m),
            Self::Internal(e) => {
                tracing::error!("{e:#}");
                (StatusCode::INTERNAL_SERVER_ERROR, "internal error".into())
            }
        };
        (status, Json(serde_json::json!({ "detail": detail }))).into_response()
    }
}

impl From<ActionError> for ApiError {
    fn from(e: ActionError) -> Self {
        match e {
            ActionError::Offline(m) => Self::Unavailable(m),
            ActionError::NoZone(n) => Self::NotFound(format!("No zone {n}")),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(e: anyhow::Error) -> Self {
        Self::Internal(e)
    }
}

type ApiResult<T> = Result<Json<T>, ApiError>;

// state

#[derive(Serialize, TS)]
#[ts(export)]
pub struct StateResponse {
    pub now: f64,
    pub build: BuildInfo,
    pub airtouch: DeviceInfo,
    pub ac: Option<AcState>,
    pub zones: Vec<ZoneState>,
    pub controller: ControllerState,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct BuildInfo {
    pub version: String,
    pub commit: Option<String>,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct AcState {
    pub on: bool,
    pub mode: String,
    pub direction: Direction,
    pub fan: Fan,
    pub setpoint: Option<f64>,
    pub temperature: Option<f64>,
    pub spill: bool,
    pub error_code: u16,
    pub fans: Vec<Fan>,
    pub limits: Limits,
    /// When the AC last switched on or off, if known.
    pub power_since: Option<f64>,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct Limits {
    pub cool: [f64; 2],
    pub heat: [f64; 2],
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct ZoneState {
    #[serde(flatten)]
    #[ts(flatten)]
    pub reading: ZoneReading,
    /// The target decisions use: the AirTouch setpoint, or ours for percentage zones.
    pub target: Option<f64>,
    pub error: Option<f64>,
    pub active: bool,
    /// The zone furthest from target, which sets fan and setpoint.
    pub driving: bool,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct ControllerState {
    pub status: Status,
    pub reason: String,
    /// From settings, not the last decision: there may not be one yet.
    pub dry_run: bool,
    pub command: Option<CommandView>,
    pub worst: Option<ZoneEval>,
    pub hold_seconds: f64,
    pub enabled: bool,
    pub parked: bool,
    pub paused_until: Option<f64>,
    pub pause_reason: Option<String>,
    pub next_tick_at: Option<f64>,
    pub tick_seconds: f64,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct CommandView {
    #[serde(flatten)]
    #[ts(flatten)]
    pub command: Command,
    pub text: String,
}

async fn build_state<D: Device>(c: &Controller<D>) -> StateResponse {
    let st = c.state().await;
    let starting = Decision::bare(Status::Starting, "Waiting for the first check");
    let d = st.decision.as_ref().unwrap_or(&starting);
    let worst = d.worst.as_ref().map(|w| w.number);
    let snap = st.snapshot.as_ref();
    StateResponse {
        now: (c.clock)(),
        build: BuildInfo {
            version: crate::version().to_owned(),
            commit: crate::commit().map(str::to_owned),
        },
        airtouch: c.device.info(),
        ac: snap.map(|s| {
            let ac = &s.ac;
            AcState {
                on: ac.on,
                mode: ac.mode.clone(),
                direction: ac.direction,
                fan: ac.fan,
                setpoint: ac.setpoint,
                temperature: ac.temperature,
                spill: ac.spill,
                error_code: ac.error_code,
                fans: ac.fans.clone(),
                limits: Limits {
                    cool: [ac.limits.cool.0, ac.limits.cool.1],
                    heat: [ac.limits.heat.0, ac.limits.heat.1],
                },
                power_since: st.runtime.last_power_change,
            }
        }),
        zones: snap.map_or_else(Vec::new, |s| {
            s.zones
                .iter()
                .map(|z| {
                    let eval = d.zones.iter().find(|e| e.number == z.number);
                    ZoneState {
                        reading: z.clone(),
                        target: eval.map_or(z.setpoint, |e| e.target),
                        error: eval.and_then(|e| e.error),
                        active: eval.is_some_and(|e| e.active),
                        driving: worst == Some(z.number),
                    }
                })
                .collect()
        }),
        controller: ControllerState {
            status: d.status,
            reason: d.reason.clone(),
            dry_run: st.settings.dry_run,
            command: d.command.map(|c| CommandView {
                command: c,
                text: summary(&c),
            }),
            worst: d.worst.clone(),
            hold_seconds: d.hold_seconds.round(),
            enabled: st.settings.enabled,
            parked: st.runtime.parked,
            paused_until: st.runtime.paused_until,
            pause_reason: st.runtime.pause_reason.clone(),
            next_tick_at: st.next_tick_at,
            tick_seconds: st.settings.tick_seconds,
        },
    }
}

async fn readyz<D: Device>(State(c): Ctl<D>) -> Result<Json<serde_json::Value>, ApiError> {
    match c.device.snapshot() {
        Some(_) => Ok(Json(serde_json::json!({ "ok": true }))),
        None => Err(ApiError::Unavailable("Not connected to AirTouch".into())),
    }
}

async fn state<D: Device>(State(c): Ctl<D>) -> Json<StateResponse> {
    Json(build_state(&c).await)
}

async fn tick<D: Device>(State(c): Ctl<D>) -> Json<StateResponse> {
    c.tick().await;
    Json(build_state(&c).await)
}

// history

#[derive(Deserialize)]
struct HistoryQuery {
    hours: Option<f64>,
    /// Seconds per point.
    step: Option<f64>,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct HistoryResponse {
    pub from: f64,
    pub to: f64,
    pub step: f64,
    pub ac: Vec<AcPoint>,
    pub zones: Vec<ZoneSeries>,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct AcPoint {
    pub ts: f64,
    pub on: bool,
    pub fan: Option<String>,
    pub setpoint: Option<f64>,
    pub temperature: Option<f64>,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct ZoneSeries {
    pub number: u8,
    pub name: String,
    pub points: Vec<ZonePoint>,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct ZonePoint {
    pub ts: f64,
    pub on: bool,
    pub temperature: Option<f64>,
    pub target: Option<f64>,
    pub damper: Option<f64>,
}

async fn history<D: Device>(
    State(c): Ctl<D>,
    Query(q): Query<HistoryQuery>,
) -> ApiResult<HistoryResponse> {
    let hours = q.hours.unwrap_or(24.0);
    if !(hours > 0.0 && hours <= 24.0 * 31.0) {
        return Err(ApiError::Invalid("hours must be within 31 days".into()));
    }
    let step = q.step.unwrap_or(if hours <= 24.0 { 60.0 } else { 600.0 });
    if step < 60.0 {
        return Err(ApiError::Invalid("step must be at least 60 seconds".into()));
    }
    let to = (c.clock)();
    let from = to - hours * 3600.0;

    // Last reading in each bucket wins.
    let bucket = |ts: f64| (ts / step).floor() as i64;
    let mut ac: BTreeMap<i64, AcPoint> = BTreeMap::new();
    let mut zones: BTreeMap<i64, BTreeMap<i64, ZonePoint>> = BTreeMap::new();
    for r in c.store.readings(from, to)? {
        if r.zone == AC_ROW {
            let p = AcPoint {
                ts: r.ts,
                on: r.on,
                fan: r.fan,
                setpoint: r.setpoint,
                temperature: r.temperature,
            };
            ac.insert(bucket(r.ts), p);
        } else {
            let p = ZonePoint {
                ts: r.ts,
                on: r.on,
                temperature: r.temperature,
                target: r.target,
                damper: r.damper,
            };
            zones.entry(r.zone).or_default().insert(bucket(r.ts), p);
        }
    }
    let names: BTreeMap<u8, String> = c
        .state()
        .await
        .snapshot
        .as_ref()
        .map(|s| s.zones.iter().map(|z| (z.number, z.name.clone())).collect())
        .unwrap_or_default();
    let zones = zones
        .into_iter()
        .filter_map(|(n, pts)| {
            let number = u8::try_from(n).ok()?;
            let name = names
                .get(&number)
                .cloned()
                .unwrap_or_else(|| format!("Zone {}", number + 1));
            Some(ZoneSeries {
                number,
                name,
                points: pts.into_values().collect(),
            })
        })
        .collect();
    Ok(Json(HistoryResponse {
        from,
        to,
        step,
        ac: ac.into_values().collect(),
        zones,
    }))
}

// activity

#[derive(Deserialize)]
struct ActivityQuery {
    limit: Option<u32>,
    before: Option<i64>,
    kind: Option<ActivityKind>,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct ActivityResponse {
    pub items: Vec<ActivityItem>,
    pub counts: BTreeMap<String, u32>,
}

async fn activity<D: Device>(
    State(c): Ctl<D>,
    Query(q): Query<ActivityQuery>,
) -> ApiResult<ActivityResponse> {
    let limit = q.limit.unwrap_or(100);
    if !(1..=500).contains(&limit) {
        return Err(ApiError::Invalid("limit must be 1-500".into()));
    }
    Ok(Json(ActivityResponse {
        items: c.store.activity(limit, q.before, q.kind)?,
        counts: c.store.activity_counts()?.into_iter().collect(),
    }))
}

// settings

/// Partial settings update. `"zones": null` means "whichever rooms are on".
#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields)]
pub struct SettingsPatch {
    pub enabled: Option<bool>,
    pub dry_run: Option<bool>,
    pub deadband: Option<f64>,
    pub fan_medium_at: Option<f64>,
    pub fan_high_at: Option<f64>,
    pub fan_hysteresis: Option<f64>,
    pub setpoint_gain: Option<f64>,
    pub max_setpoint_offset: Option<f64>,
    pub ease_within: Option<f64>,
    pub setpoint_step: Option<f64>,
    pub damper_min: Option<f64>,
    pub damper_full_at: Option<f64>,
    pub min_on_minutes: Option<f64>,
    pub min_off_minutes: Option<f64>,
    pub manual_pause_minutes: Option<f64>,
    pub tick_seconds: Option<f64>,
    #[serde(default)]
    #[ts(as = "Option<Option<Vec<u8>>>")]
    pub zones: Nullable<Vec<u8>>,
    pub targets: Option<BTreeMap<u8, f64>>,
    /// Replaces the whole map; a room left out can ask for High again.
    pub max_fan: Option<BTreeMap<u8, Fan>>,
}

/// A patch field that tells an explicit `null` apart from a missing field.
#[derive(Default)]
pub enum Nullable<T> {
    #[default]
    Absent,
    Null,
    Value(T),
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for Nullable<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(Option::<T>::deserialize(d)?.map_or(Self::Null, Self::Value))
    }
}

fn in_range(name: &str, v: Option<f64>, lo: f64, hi: f64) -> Result<(), ApiError> {
    match v {
        Some(v) if !(lo..=hi).contains(&v) => {
            Err(ApiError::Invalid(format!("{name} must be {lo}-{hi}")))
        }
        _ => Ok(()),
    }
}

impl SettingsPatch {
    fn apply(self, s: &Settings) -> Result<Settings, ApiError> {
        in_range("deadband", self.deadband, 0.5, 4.0)?;
        in_range("fan_medium_at", self.fan_medium_at, 0.25, 4.0)?;
        in_range("fan_high_at", self.fan_high_at, 0.5, 5.0)?;
        in_range("fan_hysteresis", self.fan_hysteresis, 0.0, 1.0)?;
        in_range("setpoint_gain", self.setpoint_gain, 0.0, 3.0)?;
        in_range("max_setpoint_offset", self.max_setpoint_offset, 0.0, 6.0)?;
        in_range("ease_within", self.ease_within, 0.0, 3.0)?;
        in_range("setpoint_step", self.setpoint_step, 0.5, 1.0)?;
        in_range("damper_min", self.damper_min, 0.0, 50.0)?;
        in_range("damper_full_at", self.damper_full_at, 0.5, 3.0)?;
        in_range("min_on_minutes", self.min_on_minutes, 0.0, 30.0)?;
        in_range("min_off_minutes", self.min_off_minutes, 0.0, 30.0)?;
        in_range(
            "manual_pause_minutes",
            self.manual_pause_minutes,
            0.0,
            240.0,
        )?;
        in_range("tick_seconds", self.tick_seconds, 15.0, 600.0)?;
        if let Some(targets) = &self.targets {
            for &t in targets.values() {
                in_range("target", Some(t), 10.0, 32.0)?;
            }
        }
        if let Some(caps) = &self.max_fan
            && caps.values().any(|f| f.level().is_none())
        {
            return Err(ApiError::Invalid(
                "max_fan must be low, medium or high".into(),
            ));
        }
        let mut out = s.clone();
        macro_rules! take {
            ($($f:ident),*) => { $( if let Some(v) = self.$f { out.$f = v; } )* };
        }
        take!(
            enabled,
            dry_run,
            deadband,
            fan_medium_at,
            fan_high_at,
            fan_hysteresis,
            setpoint_gain,
            max_setpoint_offset,
            ease_within,
            setpoint_step,
            damper_min,
            damper_full_at,
            min_on_minutes,
            min_off_minutes,
            manual_pause_minutes,
            tick_seconds,
            targets,
            max_fan
        );
        match self.zones {
            Nullable::Absent => {}
            Nullable::Null => out.zones = None,
            Nullable::Value(mut z) => {
                z.sort_unstable();
                z.dedup();
                out.zones = Some(z);
            }
        }
        if out.fan_medium_at >= out.fan_high_at {
            return Err(ApiError::Invalid(
                "fan_medium_at must be below fan_high_at".into(),
            ));
        }
        Ok(out)
    }
}

async fn get_settings<D: Device>(State(c): Ctl<D>) -> Json<Settings> {
    Json(c.state().await.settings.clone())
}

async fn patch_settings<D: Device>(
    State(c): Ctl<D>,
    Json(p): Json<SettingsPatch>,
) -> ApiResult<Settings> {
    let next = p.apply(&c.state().await.settings)?;
    Ok(Json(c.update_settings(next).await))
}

// actions

#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export, optional_fields)]
pub struct ZonePatch {
    pub on: Option<bool>,
    pub target: Option<f64>,
}

#[derive(Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct AcPatch {
    pub on: bool,
}

#[derive(Deserialize, TS)]
#[ts(export, optional_fields)]
pub struct PauseRequest {
    pub minutes: Option<f64>,
}

#[derive(Serialize, TS)]
#[ts(export)]
pub struct PauseResponse {
    pub paused_until: f64,
}

fn ok() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "ok": true }))
}

async fn patch_zone<D: Device>(
    State(c): Ctl<D>,
    Path(number): Path<u8>,
    Json(p): Json<ZonePatch>,
) -> ApiResult<serde_json::Value> {
    if p.on.is_none() && p.target.is_none() {
        return Err(ApiError::Invalid("send `on` and/or `target`".into()));
    }
    in_range("target", p.target, 10.0, 32.0)?;
    c.set_zone(number, p.on, p.target).await?;
    Ok(ok())
}

async fn put_ac<D: Device>(
    State(c): Ctl<D>,
    Json(p): Json<AcPatch>,
) -> ApiResult<serde_json::Value> {
    c.set_ac_power(p.on).await?;
    Ok(ok())
}

async fn pause<D: Device>(
    State(c): Ctl<D>,
    body: Option<Json<PauseRequest>>,
) -> ApiResult<PauseResponse> {
    let minutes = body.and_then(|Json(b)| b.minutes).unwrap_or(30.0);
    if !(minutes > 0.0 && minutes <= 24.0 * 60.0) {
        return Err(ApiError::Invalid("minutes must be 0-1440".into()));
    }
    Ok(Json(PauseResponse {
        paused_until: c.pause(minutes).await,
    }))
}

async fn resume<D: Device>(State(c): Ctl<D>) -> Json<serde_json::Value> {
    c.resume().await;
    ok()
}
