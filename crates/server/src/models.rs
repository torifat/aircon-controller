//! Plain data shared by the control logic, the AirTouch adapter and the API.
//!
//! Nothing here touches the network or the database, so the logic is tested with
//! hand-built snapshots. Times are Unix seconds as `f64`, the same as the API.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Direction {
    Cool,
    Heat,
    /// Dry, fan-only, or a mode we can't interpret.
    None,
}

impl Direction {
    /// +1 when a zone above target needs work (cooling), -1 for heating.
    pub fn sign(self) -> f64 {
        match self {
            Self::Cool => 1.0,
            Self::Heat => -1.0,
            Self::None => 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Fan {
    Auto,
    Low,
    Medium,
    High,
    /// Quiet, powerful, turbo or intelligent auto: not offered by this AC.
    Other,
}

pub const FAN_LEVELS: [Fan; 3] = [Fan::Low, Fan::Medium, Fan::High];

impl Fan {
    pub fn level(self) -> Option<usize> {
        FAN_LEVELS.iter().position(|&f| f == self)
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum Status {
    /// First connection to the AirTouch still in progress.
    Starting,
    /// No data from the AirTouch.
    Disconnected,
    /// The AC reports an error code: leave it alone.
    Error,
    /// Controller switched off in settings.
    Disabled,
    /// Backing off after a manual change, or paused from the UI.
    Paused,
    /// AC on but nothing to control (mode, no rooms on).
    Standby,
    /// AC off and we didn't turn it off: hands off.
    Off,
    /// AC on, steering fan and setpoint.
    Running,
    /// We turned the AC off at target and will turn it back on.
    Idle,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct ZoneReading {
    pub number: u8,
    pub name: String,
    pub on: bool,
    pub control: ZoneControl,
    /// The AirTouch's own zone setpoint.
    pub setpoint: Option<f64>,
    pub temperature: Option<f64>,
    /// Damper opening, 0.0-1.0.
    pub damper: f64,
    pub spill: bool,
    pub has_sensor: bool,
    pub low_battery: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ZoneControl {
    Temperature,
    Percentage,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SetpointLimits {
    pub cool: (f64, f64),
    pub heat: (f64, f64),
}

impl Default for SetpointLimits {
    fn default() -> Self {
        Self {
            cool: (18.0, 25.0),
            heat: (16.0, 26.0),
        }
    }
}

impl SetpointLimits {
    pub fn clamp(&self, value: f64, direction: Direction) -> f64 {
        let (lo, hi) = if direction == Direction::Heat {
            self.heat
        } else {
            self.cool
        };
        value.clamp(lo, hi)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AcReading {
    pub on: bool,
    /// AirTouch mode name, e.g. "auto_cool".
    pub mode: String,
    pub direction: Direction,
    pub fan: Fan,
    pub setpoint: Option<f64>,
    pub temperature: Option<f64>,
    pub spill: bool,
    pub error_code: u16,
    /// Fan speeds this AC accepts.
    pub fans: Vec<Fan>,
    pub limits: SetpointLimits,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Snapshot {
    pub ac: AcReading,
    pub zones: Vec<ZoneReading>,
}

/// Tunables, editable from the UI and persisted in the database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(default)]
#[ts(export)]
pub struct Settings {
    pub enabled: bool,
    /// Decisions are logged but nothing is sent to the AC. On until turned off.
    pub dry_run: bool,
    /// AC goes off when every active zone reaches target, back on when one is this
    /// many degrees past it.
    pub deadband: f64,
    /// Gap between the worst zone and its target for each fan step.
    pub fan_medium_at: f64,
    pub fan_high_at: f64,
    /// Extra margin before stepping the fan back down, so it doesn't flap.
    pub fan_hysteresis: f64,
    /// AC setpoint = AC temperature -/+ gain * worst gap, capped at max offset.
    pub setpoint_gain: f64,
    pub max_setpoint_offset: f64,
    pub setpoint_step: f64,
    pub min_on_minutes: f64,
    pub min_off_minutes: f64,
    pub manual_pause_minutes: f64,
    pub tick_seconds: f64,
    /// Zones that count; `None` means every zone that's switched on.
    pub zones: Option<Vec<u8>>,
    /// Targets for zones in percentage control, where the AirTouch app can't change the
    /// setpoint. Zones in temperature control use their AirTouch setpoint.
    pub targets: BTreeMap<u8, f64>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            dry_run: true,
            deadband: 2.0,
            fan_medium_at: 1.0,
            fan_high_at: 2.0,
            fan_hysteresis: 0.2,
            setpoint_gain: 1.0,
            max_setpoint_offset: 3.0,
            setpoint_step: 1.0,
            min_on_minutes: 5.0,
            min_off_minutes: 5.0,
            manual_pause_minutes: 30.0,
            tick_seconds: 60.0,
            zones: None,
            targets: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, TS)]
#[ts(export)]
pub struct Command {
    pub power: Option<bool>,
    pub fan: Option<Fan>,
    pub setpoint: Option<f64>,
}

impl Command {
    pub fn is_empty(&self) -> bool {
        self.power.is_none() && self.fan.is_none() && self.setpoint.is_none()
    }
}

/// What the AC should look like next tick if nobody else touches it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Expected {
    pub power: bool,
    pub mode: String,
    pub fan: Fan,
    pub setpoint: Option<f64>,
}

impl Expected {
    pub fn of(ac: &AcReading) -> Self {
        Self {
            power: ac.on,
            mode: ac.mode.clone(),
            fan: ac.fan,
            setpoint: ac.setpoint,
        }
    }

    pub fn apply(mut self, cmd: &Command) -> Self {
        self.power = cmd.power.unwrap_or(self.power);
        self.fan = cmd.fan.unwrap_or(self.fan);
        self.setpoint = cmd.setpoint.or(self.setpoint);
        self
    }
}

/// Controller memory carried between ticks and across restarts.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Runtime {
    /// The controller turned the AC off at target and owns turning it back on.
    pub parked: bool,
    pub paused_until: Option<f64>,
    pub pause_reason: Option<String>,
    pub last_power_change: Option<f64>,
    pub last_seen_power: Option<bool>,
    pub expected: Option<Expected>,
    /// The lone room we switched to percentage control and opened fully. Its target is in
    /// `Settings::targets` until it's handed back.
    pub opened: Option<u8>,
    /// We've asked for `opened` to go back to temperature control; it's kept until the
    /// console reports that it has, and asked again each check until then.
    pub handing_back: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[ts(export)]
pub struct ZoneEval {
    pub number: u8,
    pub name: String,
    /// Counts towards decisions.
    pub active: bool,
    pub target: Option<f64>,
    /// Degrees of work still needed: positive = too warm when cooling / too cold when
    /// heating; negative = past target.
    pub error: Option<f64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
#[ts(export)]
pub enum ActivityKind {
    Decision,
    Info,
    External,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub kind: ActivityKind,
    pub title: String,
    pub reason: String,
}

impl Event {
    pub fn new(kind: ActivityKind, title: impl Into<String>, reason: impl Into<String>) -> Self {
        Self {
            kind,
            title: title.into(),
            reason: reason.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Decision {
    pub status: Status,
    pub reason: String,
    pub command: Option<Command>,
    pub worst: Option<ZoneEval>,
    pub zones: Vec<ZoneEval>,
    pub events: Vec<Event>,
    pub dry_run: bool,
    /// Seconds until a min on/off time allows the next power change.
    pub hold_seconds: f64,
}

impl Decision {
    pub fn bare(status: Status, reason: impl Into<String>) -> Self {
        Self {
            status,
            reason: reason.into(),
            command: None,
            worst: None,
            zones: Vec::new(),
            events: Vec::new(),
            dry_run: false,
            hold_seconds: 0.0,
        }
    }
}
