//! The control loop's brain: one pure function from (snapshot, settings, memory) to a decision.
//!
//! The AirTouch closes dampers as rooms reach target but the Daikin keeps blowing at full
//! tilt, so the surplus lands in the spill zone. Instead we throttle the AC itself, driven
//! by whichever active zone is furthest from its target:
//!
//! - fan steps Low / Medium / High with the size of that gap;
//! - the AC setpoint is pulled away from the AC's own reading by the gap, so the inverter
//!   compressor works hard when rooms are far off, and sits at that reading within
//!   `ease_within` of target so it idles;
//! - when every active zone reaches target the AC is turned off ("parked"), and turned back
//!   on once any zone drifts `deadband` degrees past its target;
//! - min on/off times protect the compressor;
//! - a fan/setpoint/mode change made outside the controller pauses it for a while.
//!
//! Rooms run in percentage control with `damper_moves()` setting their openings: the
//! AirTouch resets the AC setpoint to the room targets while any room is in temperature
//! control, and its own damper logic spills whatever it throttles.

use crate::models::{
    AcReading, ActivityKind, Command, Decision, Direction, Event, Expected, FAN_LEVELS, Fan,
    Runtime, Settings, Snapshot, Status, ZoneControl, ZoneEval, ZoneReading,
};

const SETPOINT_TOLERANCE: f64 = 0.05;

fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

pub fn evaluate_zones(snap: &Snapshot, s: &Settings) -> Vec<ZoneEval> {
    let sign = snap.ac.direction.sign();
    snap.zones
        .iter()
        .map(|z| {
            let target = match (z.control, s.targets.get(&z.number)) {
                (ZoneControl::Percentage, Some(&t)) => Some(t),
                _ => z.setpoint,
            };
            let counted = s.zones.as_ref().is_none_or(|zs| zs.contains(&z.number));
            let error = match (z.temperature, target) {
                (Some(t), Some(target)) if sign != 0.0 => Some(round2(sign * (t - target))),
                _ => None,
            };
            ZoneEval {
                number: z.number,
                name: z.name.clone(),
                active: z.on && counted && z.temperature.is_some() && target.is_some(),
                target,
                error,
            }
        })
        .collect()
}

/// Fan level for the worst zone's gap, stepping down only past a hysteresis margin.
///
/// Within `ease_within` it is always Low: the compressor idles there, and a room that
/// close would otherwise get a strong draft through its wide-open damper.
pub fn fan_for(gap: f64, current: Fan, s: &Settings, available: &[Fan]) -> Fan {
    if gap < s.ease_within {
        return Fan::Low;
    }
    let thresholds = [0.0, s.fan_medium_at, s.fan_high_at];
    let mut level = thresholds.iter().rposition(|&t| gap >= t).unwrap_or(0);
    // Going down: hold the current level until we're clearly below its threshold.
    if let Some(cur) = current.level()
        && level < cur
        && gap >= thresholds[cur] - s.fan_hysteresis
    {
        level = cur;
    }
    // Fall back to the nearest slower speed if the AC doesn't offer this one.
    while level > 0 && !available.contains(&FAN_LEVELS[level]) {
        level -= 1;
    }
    FAN_LEVELS[level]
}

/// AC setpoint that makes the inverter work in proportion to the worst gap.
///
/// Within `ease_within` of target it sits at the AC's own reading instead, rounded away
/// from work, so the compressor idles: on a Low fan a working compressor sends out a
/// trickle of very cold (or hot) air.
pub fn setpoint_for(ac: &AcReading, worst: &ZoneEval, s: &Settings) -> f64 {
    let reference = ac.temperature.or(worst.target).unwrap_or(24.0);
    let gap = worst.error.unwrap_or(0.0).max(0.0);
    let steps = if gap < s.ease_within {
        let steps = reference / s.setpoint_step;
        if ac.direction == Direction::Heat {
            steps.floor()
        } else {
            steps.ceil()
        }
    } else {
        let offset = (s.setpoint_gain * gap).min(s.max_setpoint_offset);
        ((reference - ac.direction.sign() * offset) / s.setpoint_step).round()
    };
    ac.limits.clamp(steps * s.setpoint_step, ac.direction)
}

/// Fan + setpoint command with only the fields that change (all of them when `force`).
pub fn regulate(ac: &AcReading, worst: &ZoneEval, s: &Settings, force: bool) -> Command {
    let fan = fan_for(worst.error.unwrap_or(0.0), ac.fan, s, &ac.fans);
    let setpoint = setpoint_for(ac, worst, s);
    let same_setpoint = ac
        .setpoint
        .is_some_and(|cur| (setpoint - cur).abs() < s.setpoint_step / 2.0);
    Command {
        power: None,
        fan: (force || fan != ac.fan).then_some(fan),
        setpoint: (force || !same_setpoint).then_some(setpoint),
    }
}

fn hold(rt: &Runtime, minutes: f64, now: f64) -> f64 {
    rt.last_power_change
        .map_or(0.0, |t| (t + minutes * 60.0 - now).max(0.0))
}

fn gap_text(z: &ZoneEval, direction: Direction) -> String {
    // `error` is signed by direction: cooling's positive gap is a room above target,
    // heating's is a room below it.
    let err = z.error.unwrap_or(0.0);
    let above = if direction == Direction::Heat {
        err < 0.0
    } else {
        err >= 0.0
    };
    let word = if above { "above" } else { "below" };
    format!("{} is {:.1}° {word} target", z.name, err.abs())
}

enum ExternalChange {
    PowerOff,
    PowerOn,
    Settings(String),
}

/// Compare what we left the AC as with what it reports now.
fn external_change(expected: Option<&Expected>, ac: &AcReading) -> Option<ExternalChange> {
    let exp = expected?;
    match (exp.power, ac.on) {
        (true, false) => return Some(ExternalChange::PowerOff),
        (false, true) => return Some(ExternalChange::PowerOn),
        (false, false) => return None,
        (true, true) => {}
    }
    let mut diffs = Vec::new();
    if exp.mode != ac.mode {
        diffs.push(format!("mode {} → {}", exp.mode, ac.mode));
    }
    if exp.fan != ac.fan {
        diffs.push(format!(
            "fan {} → {}",
            exp.fan.title().to_lowercase(),
            ac.fan.title().to_lowercase()
        ));
    }
    if let (Some(a), Some(b)) = (exp.setpoint, ac.setpoint)
        && (a - b).abs() > SETPOINT_TOLERANCE
    {
        diffs.push(format!("setpoint {a}° → {b}°"));
    }
    (!diffs.is_empty()).then(|| ExternalChange::Settings(diffs.join(", ")))
}

struct Step<'a> {
    snap: &'a Snapshot,
    s: &'a Settings,
    rt: Runtime,
    now: f64,
    zones: Vec<ZoneEval>,
    events: Vec<Event>,
}

impl Step<'_> {
    fn done(
        mut self,
        status: Status,
        reason: impl Into<String>,
        cmd: Option<Command>,
        worst: Option<ZoneEval>,
        hold_seconds: f64,
        watch: bool,
    ) -> (Decision, Runtime) {
        let cmd = cmd.filter(|c| !c.is_empty());
        let mut expected = Expected::of(&self.snap.ac);
        if let Some(c) = &cmd
            && !self.s.dry_run
        {
            expected = expected.apply(c);
            if let Some(power) = c.power {
                self.rt.last_power_change = Some(self.now);
                self.rt.parked = !power;
            }
        }
        self.rt.expected = watch.then_some(expected);
        let decision = Decision {
            status,
            reason: reason.into(),
            command: cmd,
            worst,
            zones: self.zones,
            events: self.events,
            dry_run: self.s.dry_run,
            hold_seconds,
        };
        (decision, self.rt)
    }

    fn simple(self, status: Status, reason: impl Into<String>) -> (Decision, Runtime) {
        self.done(status, reason, None, None, 0.0, true)
    }
}

pub fn decide(snap: &Snapshot, s: &Settings, rt: &Runtime, now: f64) -> (Decision, Runtime) {
    let ac = &snap.ac;
    let mut rt = rt.clone();

    // Power flipped without us asking for it: restart the min on/off clock. Our own
    // commands already stamped it when they were sent.
    let predicted = rt.expected.as_ref().is_some_and(|e| e.power == ac.on);
    if rt.last_seen_power.is_some_and(|p| p != ac.on) && !predicted {
        rt.last_power_change = Some(now);
    }
    rt.last_seen_power = Some(ac.on);

    let mut step = Step {
        snap,
        s,
        rt,
        now,
        zones: evaluate_zones(snap, s),
        events: Vec::new(),
    };

    if !s.enabled {
        step.rt.parked = false;
        return step.done(
            Status::Disabled,
            "Controller is off; AirTouch runs on its own",
            None,
            None,
            0.0,
            false,
        );
    }

    match external_change(step.rt.expected.as_ref(), ac) {
        Some(ExternalChange::Settings(text)) => {
            step.rt.parked = false;
            step.rt.paused_until = Some(now + s.manual_pause_minutes * 60.0);
            step.rt.pause_reason = Some(format!("Manual change: {text}"));
            step.events.push(Event::new(
                ActivityKind::External,
                format!("Manual change: {text}"),
                format!(
                    "Made outside Climate; pausing {} min",
                    s.manual_pause_minutes
                ),
            ));
        }
        Some(change) => {
            step.rt.parked = false;
            let title = if matches!(change, ExternalChange::PowerOff) {
                "AC turned off"
            } else {
                "AC turned on"
            };
            step.events.push(Event::new(
                ActivityKind::External,
                title,
                "Made outside Climate; handing control back",
            ));
        }
        None => {}
    }

    if let Some(until) = step.rt.paused_until {
        if now < until {
            let reason = step
                .rt
                .pause_reason
                .clone()
                .unwrap_or_else(|| "Paused".into());
            return step.done(Status::Paused, reason, None, None, 0.0, false);
        }
        step.rt.paused_until = None;
        step.rt.pause_reason = None;
        step.events.push(Event::new(
            ActivityKind::Info,
            "Controller resumed",
            "Pause ended",
        ));
    }

    if ac.error_code != 0 {
        step.rt.parked = false;
        return step.simple(
            Status::Error,
            format!("AC reports error {}; not adjusting", ac.error_code),
        );
    }

    if ac.direction == Direction::None {
        return step.simple(
            Status::Standby,
            format!("Mode {} isn't controlled", ac.mode),
        );
    }

    let Some(worst) = step
        .zones
        .iter()
        .filter(|z| z.active)
        .max_by(|a, b| a.error.unwrap_or(0.0).total_cmp(&b.error.unwrap_or(0.0)))
        .cloned()
    else {
        if step.rt.parked {
            step.rt.parked = false;
            step.events.push(Event::new(
                ActivityKind::Info,
                "Session ended",
                "No rooms on; leaving the AC off",
            ));
        }
        return if ac.on {
            step.simple(Status::Standby, "No rooms on")
        } else {
            step.simple(Status::Off, "AC is off")
        };
    };
    let gap = worst.error.unwrap_or(0.0);
    let text = gap_text(&worst, ac.direction);

    if ac.on {
        if gap <= 0.0 {
            let held = hold(&step.rt, s.min_on_minutes, now);
            if held > 0.0 {
                let cmd = regulate(ac, &worst, s, false);
                return step.done(
                    Status::Running,
                    "All rooms at target; keeping the AC on for its minimum run time",
                    Some(cmd),
                    Some(worst),
                    held,
                    true,
                );
            }
            return step.done(
                Status::Idle,
                format!("All rooms at target ({text}); turning the AC off"),
                Some(Command {
                    power: Some(false),
                    ..Command::default()
                }),
                Some(worst),
                0.0,
                true,
            );
        }
        let cmd = regulate(ac, &worst, s, false);
        return step.done(Status::Running, text, Some(cmd), Some(worst), 0.0, true);
    }

    if !step.rt.parked {
        return step.simple(Status::Off, "AC is off");
    }

    let held = hold(&step.rt, s.min_off_minutes, now);
    if gap < s.deadband {
        let reason = format!("All rooms within {}° of target ({text})", s.deadband);
        return step.done(Status::Idle, reason, None, Some(worst), held, true);
    }
    if held > 0.0 {
        let reason = format!("{text}; waiting out the AC's minimum off time");
        return step.done(Status::Idle, reason, None, Some(worst), held, true);
    }
    let cmd = Command {
        power: Some(true),
        ..regulate(ac, &worst, s, true)
    };
    step.done(
        Status::Running,
        format!("{text}; turning the AC back on"),
        Some(cmd),
        Some(worst),
        0.0,
        true,
    )
}

/// A damper the controller wants moved, and the room switched to percentage control first
/// if it isn't in it yet.
#[derive(Debug, Clone, PartialEq)]
pub struct DamperMove {
    pub zone: u8,
    pub name: String,
    /// Opening now, 0.0-1.0.
    pub from: f64,
    /// Opening wanted, in percent.
    pub to: u8,
    /// The room is in temperature control and gets switched to percentage control.
    pub switch: bool,
    /// Its AirTouch setpoint, which becomes the target we keep when it's switched.
    pub setpoint: Option<f64>,
}

/// Damper movements are only sent past this many percentage points, or to reach a fully
/// open or fully throttled room, so small wobbles in a reading don't move them every check.
const DAMPER_SETTLE: f64 = 10.0;

/// Opening for a room `error` degrees from target (positive = needs work): the minimum at
/// or past target, fully open from `damper_full_at` away, in 5% steps between.
pub fn opening_for(error: f64, s: &Settings) -> u8 {
    let min = s.damper_min.clamp(0.0, 100.0);
    let share = (error / s.damper_full_at).clamp(0.0, 1.0);
    ((min + (100.0 - min) * share) / 5.0).round() as u8 * 5
}

/// Openings across the rooms that are on must add up to this, or the AirTouch spills.
const NO_SPILL_TOTAL: f64 = 100.0;

/// The damper moves to make this check.
///
/// With a temperature-control room on, the AirTouch keeps resetting the AC setpoint to the
/// room targets, so every room is switched to percentage control (its setpoint becomes the
/// target kept here) and its damper follows its gap, closing down to `damper_min` as it
/// reaches target. The room needing it most opens as far as its own gap asks, and at least
/// far enough that the rooms on add up to `NO_SPILL_TOTAL`, so nothing spills: with Mouni
/// at 20%, Study near target sits at 80% instead of taking a full blast. Rooms that are off,
/// have no sensor or don't count keep their opening, once in percentage control.
pub fn damper_moves(snap: &Snapshot, zones: &[ZoneEval], s: &Settings) -> Vec<DamperMove> {
    let neediest = zones
        .iter()
        .filter(|z| z.active)
        .max_by(|a, b| a.error.unwrap_or(0.0).total_cmp(&b.error.unwrap_or(0.0)))
        .map(|z| z.number);
    let aim = |z: &ZoneReading, wanted: Option<u8>, opening: bool| {
        let switch = z.control == ZoneControl::Temperature;
        let now = z.damper * 100.0;
        let to = match wanted {
            Some(to) => to,
            // Not ours to aim yet: switch it fully open, which never causes spill.
            None if switch => 100,
            None => return None,
        };
        let at_end = to == 100 || f64::from(to) <= s.damper_min;
        let moved = (f64::from(to) - now).abs();
        // The neediest room opening up is never held back: short of it, the total spills.
        let needed = switch
            || moved >= DAMPER_SETTLE
            || (at_end && moved >= 2.5)
            || (opening && f64::from(to) > now);
        Some(DamperMove {
            zone: z.number,
            name: z.name.clone(),
            from: z.damper,
            to,
            switch,
            setpoint: z.setpoint,
        })
        .filter(|_| needed)
    };
    // Every other room first: the neediest makes up what they leave.
    let mut others = 0.0;
    let mut moves: Vec<DamperMove> = Vec::new();
    for z in snap.zones.iter().filter(|z| Some(z.number) != neediest) {
        let eval = zones.iter().find(|e| e.number == z.number);
        let wanted = eval
            .filter(|e| e.active)
            .and_then(|e| e.error.map(|err| opening_for(err, s)));
        let mv = aim(z, wanted, false);
        if z.on {
            others += mv.as_ref().map_or(z.damper * 100.0, |m| f64::from(m.to));
        }
        moves.extend(mv);
    }
    if let Some(z) = snap.zones.iter().find(|z| Some(z.number) == neediest) {
        let err = zones
            .iter()
            .find(|e| e.number == z.number)
            .and_then(|e| e.error);
        let own = opening_for(err.unwrap_or(0.0), s);
        let fill = ((NO_SPILL_TOTAL - others).clamp(0.0, 100.0) / 5.0).ceil() as u8 * 5;
        moves.extend(aim(z, Some(own.max(fill)), true));
    }
    let order = |n: u8| snap.zones.iter().position(|z| z.number == n);
    moves.sort_by_key(|m| order(m.zone));
    moves
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::SetpointLimits;

    const NOW: f64 = 1_000_000.0;

    fn live() -> Settings {
        Settings {
            dry_run: false,
            ..Settings::default()
        }
    }

    fn zone(number: u8, temp: f64) -> ZoneReading {
        ZoneReading {
            number,
            name: format!("Zone {number}"),
            on: true,
            control: ZoneControl::Temperature,
            setpoint: Some(24.0),
            temperature: Some(temp),
            damper: 0.5,
            spill: false,
            has_sensor: true,
            low_battery: false,
        }
    }

    fn named(name: &str, z: ZoneReading) -> ZoneReading {
        ZoneReading {
            name: name.into(),
            ..z
        }
    }

    fn ac(mode: &str) -> AcReading {
        let direction = match mode {
            "cool" | "auto_cool" => Direction::Cool,
            "heat" | "auto_heat" => Direction::Heat,
            _ => Direction::None,
        };
        AcReading {
            on: true,
            mode: mode.into(),
            direction,
            fan: Fan::Low,
            setpoint: Some(24.0),
            temperature: Some(24.0),
            spill: false,
            error_code: 0,
            fans: FAN_LEVELS.to_vec(),
            limits: SetpointLimits::default(),
        }
    }

    fn cool() -> AcReading {
        ac("cool")
    }

    fn snap(ac: AcReading, zones: Vec<ZoneReading>) -> Snapshot {
        Snapshot { ac, zones }
    }

    fn run(s: &Snapshot, settings: &Settings, rt: &Runtime) -> (Decision, Runtime) {
        decide(s, settings, rt, NOW)
    }

    fn expected(power: bool) -> Option<Expected> {
        Some(Expected {
            power,
            mode: "cool".into(),
            fan: Fan::Low,
            setpoint: Some(24.0),
        })
    }

    fn cmd(power: Option<bool>, fan: Option<Fan>, setpoint: Option<f64>) -> Option<Command> {
        Some(Command {
            power,
            fan,
            setpoint,
        })
    }

    #[test]
    fn fan_levels_with_hysteresis() {
        let s = Settings::default();
        let cases = [
            (0.3, Fan::Low, Fan::Low),
            (1.2, Fan::Low, Fan::Medium),
            (2.9, Fan::Low, Fan::High),
            (2.9, Fan::Auto, Fan::High),
            (1.9, Fan::High, Fan::High),
            (1.7, Fan::High, Fan::Medium),
            (0.9, Fan::Medium, Fan::Low),
            (0.5, Fan::Medium, Fan::Low),
        ];
        for (gap, current, want) in cases {
            assert_eq!(
                fan_for(gap, current, &s, &FAN_LEVELS),
                want,
                "gap {gap} from {current:?}"
            );
        }
        assert_eq!(
            fan_for(2.5, Fan::Low, &s, &[Fan::Low, Fan::Medium]),
            Fan::Medium
        );
        // Without an ease-off band the hysteresis holds Medium just under its threshold.
        let s = Settings {
            ease_within: 0.0,
            ..Settings::default()
        };
        assert_eq!(fan_for(0.9, Fan::Medium, &s, &FAN_LEVELS), Fan::Medium);
    }

    #[test]
    fn far_from_target_runs_hard() {
        // The morning's real state: Study 26.9 vs 24, AC thinks it's at 24.
        let s = snap(cool(), vec![zone(2, 24.7), named("Study", zone(3, 26.9))]);
        let (d, _) = run(&s, &live(), &Runtime::default());
        assert_eq!(d.status, Status::Running);
        assert_eq!(d.worst.unwrap().name, "Study");
        assert_eq!(d.command, cmd(None, Some(Fan::High), Some(21.0)));
        assert!(
            d.reason.contains("Study is 2.9° above target"),
            "{}",
            d.reason
        );
    }

    #[test]
    fn near_target_eases_off() {
        let s = snap(
            AcReading {
                fan: Fan::High,
                setpoint: Some(21.0),
                ..cool()
            },
            vec![zone(3, 24.4)],
        );
        let (d, _) = run(&s, &live(), &Runtime::default());
        assert_eq!(d.command, cmd(None, Some(Fan::Low), Some(24.0)));
    }

    #[test]
    fn no_command_when_already_right() {
        let (d, _) = run(
            &snap(cool(), vec![zone(3, 24.3)]),
            &live(),
            &Runtime::default(),
        );
        assert_eq!(d.command, None);
    }

    #[test]
    fn setpoint_respects_limits() {
        let ac = AcReading {
            temperature: Some(19.0),
            setpoint: Some(19.0),
            ..cool()
        };
        let (d, _) = run(&snap(ac, vec![zone(3, 27.0)]), &live(), &Runtime::default());
        assert_eq!(d.command.unwrap().setpoint, Some(18.0));
    }

    #[test]
    fn off_zones_are_ignored() {
        let off = ZoneReading {
            on: false,
            ..zone(0, 30.0)
        };
        let (d, _) = run(
            &snap(cool(), vec![off, zone(3, 24.3)]),
            &live(),
            &Runtime::default(),
        );
        assert_eq!(d.worst.unwrap().number, 3);
    }

    #[test]
    fn zone_filter() {
        let settings = Settings {
            zones: Some(vec![2]),
            ..live()
        };
        let (d, _) = run(
            &snap(cool(), vec![zone(2, 24.5), zone(3, 27.0)]),
            &settings,
            &Runtime::default(),
        );
        assert_eq!(d.worst.unwrap().number, 2);
    }

    #[test]
    fn percentage_zone_uses_controller_target() {
        let settings = Settings {
            targets: [(3, 26.0)].into(),
            ..live()
        };
        let z = ZoneReading {
            control: ZoneControl::Percentage,
            ..zone(3, 26.9)
        };
        let (d, _) = run(&snap(cool(), vec![z]), &settings, &Runtime::default());
        let worst = d.worst.unwrap();
        assert_eq!(worst.target, Some(26.0));
        assert_eq!(worst.error, Some(0.9));
    }

    #[test]
    fn heating_mirrors_cooling() {
        let ac = AcReading {
            temperature: Some(21.0),
            setpoint: Some(21.0),
            ..ac("heat")
        };
        let z = ZoneReading {
            setpoint: Some(21.0),
            ..zone(1, 19.6)
        };
        let (d, _) = run(&snap(ac, vec![z]), &live(), &Runtime::default());
        assert_eq!(d.status, Status::Running);
        assert_eq!(d.command, cmd(None, Some(Fan::Medium), Some(22.0)));
        assert!(d.reason.contains("1.4° below target"), "{}", d.reason);
    }

    #[test]
    fn overshoot_reads_naturally() {
        let (d, _) = run(
            &snap(cool(), vec![zone(3, 23.5)]),
            &live(),
            &Runtime::default(),
        );
        assert!(d.reason.contains("0.5° below target"), "{}", d.reason);
        let heat = AcReading {
            temperature: Some(21.0),
            ..ac("heat")
        };
        let (d, _) = run(
            &snap(heat, vec![zone(3, 24.5)]),
            &live(),
            &Runtime::default(),
        );
        assert!(d.reason.contains("0.5° above target"), "{}", d.reason);
    }

    #[test]
    fn turns_off_at_target() {
        let (d, rt) = run(
            &snap(cool(), vec![zone(2, 23.9), zone(3, 24.0)]),
            &live(),
            &Runtime::default(),
        );
        assert_eq!(d.status, Status::Idle);
        assert_eq!(d.command, cmd(Some(false), None, None));
        assert!(rt.parked);
        assert_eq!(rt.last_power_change, Some(NOW));
    }

    #[test]
    fn min_on_time_holds() {
        let rt = Runtime {
            last_power_change: Some(NOW - 60.0),
            last_seen_power: Some(true),
            ..Runtime::default()
        };
        let (d, _) = run(&snap(cool(), vec![zone(3, 23.8)]), &live(), &rt);
        assert_eq!(d.status, Status::Running);
        assert!(d.command.is_none_or(|c| c.power.is_none()));
        assert_eq!(d.hold_seconds, 240.0);
    }

    fn parked(since: f64) -> Runtime {
        Runtime {
            parked: true,
            last_power_change: Some(NOW - since),
            last_seen_power: Some(false),
            expected: expected(false),
            ..Runtime::default()
        }
    }

    #[test]
    fn stays_parked_inside_deadband() {
        let off = AcReading {
            on: false,
            ..cool()
        };
        let (d, rt) = run(&snap(off, vec![zone(3, 25.5)]), &live(), &parked(3600.0));
        assert_eq!(d.status, Status::Idle);
        assert_eq!(d.command, None);
        assert!(rt.parked);
    }

    #[test]
    fn resumes_past_deadband() {
        let off = AcReading {
            on: false,
            temperature: Some(26.0),
            ..cool()
        };
        let (d, rt) = run(&snap(off, vec![zone(3, 26.1)]), &live(), &parked(3600.0));
        assert_eq!(d.status, Status::Running);
        let c = d.command.unwrap();
        assert_eq!((c.power, c.fan), (Some(true), Some(Fan::High)));
        assert!(!rt.parked);
    }

    #[test]
    fn resume_waits_for_min_off_time() {
        let off = AcReading {
            on: false,
            ..cool()
        };
        let (d, _) = run(&snap(off, vec![zone(3, 26.5)]), &live(), &parked(60.0));
        assert_eq!(d.command, None);
        assert_eq!(d.hold_seconds, 240.0);
    }

    #[test]
    fn never_turns_on_an_ac_the_user_turned_off() {
        let off = AcReading {
            on: false,
            ..cool()
        };
        let (d, _) = run(
            &snap(off, vec![zone(3, 30.0)]),
            &live(),
            &Runtime::default(),
        );
        assert_eq!(d.status, Status::Off);
        assert_eq!(d.command, None);
    }

    #[test]
    fn no_rooms_on_ends_session() {
        let off = AcReading {
            on: false,
            ..cool()
        };
        let z = ZoneReading {
            on: false,
            ..zone(3, 30.0)
        };
        let (d, rt) = run(&snap(off, vec![z]), &live(), &parked(3600.0));
        assert_eq!(d.status, Status::Off);
        assert!(!rt.parked);
    }

    #[test]
    fn fan_change_pauses() {
        let rt = Runtime {
            last_seen_power: Some(true),
            expected: expected(true),
            ..Runtime::default()
        };
        let s = snap(
            AcReading {
                fan: Fan::High,
                ..cool()
            },
            vec![zone(3, 26.0)],
        );
        let (d, rt) = run(&s, &live(), &rt);
        assert_eq!(d.status, Status::Paused);
        assert_eq!(d.command, None);
        assert_eq!(rt.paused_until, Some(NOW + 1800.0));
        assert_eq!(d.events[0].kind, ActivityKind::External);
        assert!(
            d.events[0].title.contains("fan low → high"),
            "{}",
            d.events[0].title
        );
    }

    #[test]
    fn pause_expires() {
        let rt = Runtime {
            paused_until: Some(NOW - 1.0),
            pause_reason: Some("x".into()),
            last_seen_power: Some(true),
            ..Runtime::default()
        };
        let (d, rt) = run(&snap(cool(), vec![zone(3, 26.0)]), &live(), &rt);
        assert_eq!(d.status, Status::Running);
        assert_eq!(rt.paused_until, None);
        assert_eq!(d.events[0].title, "Controller resumed");
    }

    #[test]
    fn user_turning_off_is_respected() {
        let rt = Runtime {
            last_seen_power: Some(true),
            expected: expected(true),
            ..Runtime::default()
        };
        let off = snap(
            AcReading {
                on: false,
                ..cool()
            },
            vec![zone(3, 28.0)],
        );
        let (d, rt) = run(&off, &live(), &rt);
        assert_eq!(d.status, Status::Off);
        assert_eq!(rt.last_power_change, Some(NOW));
        let (d, _) = decide(&off, &live(), &rt, NOW + 3600.0);
        assert_eq!(d.command, None);
    }

    #[test]
    fn our_own_command_is_not_external() {
        let (_, rt) = run(
            &snap(cool(), vec![zone(3, 26.9)]),
            &live(),
            &Runtime::default(),
        );
        let after = snap(
            AcReading {
                fan: Fan::High,
                setpoint: Some(21.0),
                ..cool()
            },
            vec![zone(3, 26.5)],
        );
        let (d, _) = decide(&after, &live(), &rt, NOW + 60.0);
        assert_eq!(d.status, Status::Running);
        assert!(d.events.is_empty());
    }

    #[test]
    fn dry_run_decides_but_keeps_state() {
        let (d, rt) = run(
            &snap(cool(), vec![zone(3, 23.5)]),
            &Settings::default(),
            &Runtime::default(),
        );
        assert!(d.dry_run);
        assert_eq!(d.command, cmd(Some(false), None, None));
        assert!(!rt.parked);
        assert!(rt.expected.unwrap().power);
    }

    #[test]
    fn guards() {
        let disabled = Settings {
            enabled: false,
            ..live()
        };
        let (d, _) = run(
            &snap(cool(), vec![zone(3, 28.0)]),
            &disabled,
            &Runtime::default(),
        );
        assert_eq!((d.status, d.command), (Status::Disabled, None));

        let broken = AcReading {
            error_code: 5,
            ..cool()
        };
        let (d, _) = run(
            &snap(broken, vec![zone(3, 28.0)]),
            &live(),
            &Runtime::default(),
        );
        assert_eq!((d.status, d.command), (Status::Error, None));

        let (d, _) = run(
            &snap(ac("fan"), vec![zone(3, 28.0)]),
            &live(),
            &Runtime::default(),
        );
        assert_eq!(d.status, Status::Standby);
    }

    fn off(z: ZoneReading) -> ZoneReading {
        ZoneReading { on: false, ..z }
    }

    fn pct(z: ZoneReading, open: f64) -> ZoneReading {
        ZoneReading {
            control: ZoneControl::Percentage,
            damper: open,
            ..z
        }
    }

    fn moves(s: &Snapshot) -> Vec<(u8, u8, bool)> {
        let settings = live();
        damper_moves(s, &evaluate_zones(s, &settings), &settings)
            .into_iter()
            .map(|m| (m.zone, m.to, m.switch))
            .collect()
    }

    #[test]
    fn openings_follow_the_gap() {
        let s = live();
        assert_eq!(opening_for(-0.5, &s), 10);
        assert_eq!(opening_for(0.0, &s), 10);
        assert_eq!(opening_for(0.5, &s), 55);
        assert_eq!(opening_for(0.3, &s), 35);
        assert_eq!(opening_for(1.0, &s), 100);
        assert_eq!(opening_for(2.5, &s), 100);
    }

    #[test]
    fn neediest_room_makes_up_the_rest_and_others_close_down() {
        // This afternoon: Study 0.8° over, Mouni 0.5° under at 100% and getting colder.
        // Study's gap asks for 80%; Mouni closing to 10% leaves it 90% to fill.
        let s = snap(
            cool(),
            vec![
                pct(named("Study", zone(3, 24.8)), 0.85),
                pct(named("Mouni", zone(2, 23.5)), 1.0),
                off(pct(zone(1, 25.3), 0.05)),
            ],
        );
        assert_eq!(moves(&s), vec![(3, 90, false), (2, 10, false)]);
        // Far off, it's fully open whatever the others leave.
        let s = snap(
            cool(),
            vec![pct(zone(3, 25.5), 0.9), pct(zone(2, 24.6), 0.6)],
        );
        assert_eq!(moves(&s), vec![(3, 100, false)]);
    }

    #[test]
    fn neediest_room_near_target_shares_the_air() {
        // 18:31 on Nov 2: Study 0.3° over at 100%, Mouni held at 20%. Study's gap asks for
        // 35%, but 80% is what keeps the total at 100 so nothing spills.
        let s = snap(
            cool(),
            vec![pct(zone(3, 24.3), 1.0), pct(zone(2, 24.1), 0.2)],
        );
        assert_eq!(moves(&s), vec![(3, 80, false)]);
        // Opening up to cover the total is never held back as a wobble.
        let s = snap(
            cool(),
            vec![pct(zone(3, 24.3), 0.75), pct(zone(2, 24.1), 0.2)],
        );
        assert_eq!(moves(&s), vec![(3, 80, false)]);
    }

    #[test]
    fn small_wobbles_dont_move_dampers() {
        let s = snap(
            cool(),
            vec![pct(zone(3, 25.5), 1.0), pct(zone(2, 24.3), 0.40)],
        );
        // Zone 2 wants 35%: 5 points off isn't worth a move.
        assert_eq!(moves(&s), vec![]);
        // But closing the last bit to the minimum is.
        let s = snap(
            cool(),
            vec![pct(zone(3, 25.5), 1.0), pct(zone(2, 23.9), 0.15)],
        );
        assert_eq!(moves(&s), vec![(2, 10, false)]);
    }

    #[test]
    fn temperature_control_rooms_are_switched() {
        let s = snap(
            cool(),
            vec![zone(3, 24.8), off(zone(1, 24.0)), pct(zone(2, 24.0), 0.1)],
        );
        // On: aimed like any room, filling what zone 2's 10% leaves. Off: switched fully
        // open, which never spills.
        assert_eq!(moves(&s), vec![(3, 90, true), (1, 100, true)]);
    }

    #[test]
    fn rooms_that_dont_count_are_left_alone() {
        let s = snap(
            cool(),
            vec![pct(zone(3, 25.0), 1.0), pct(zone(2, 22.0), 0.6)],
        );
        let settings = Settings {
            zones: Some(vec![3]),
            ..live()
        };
        assert!(damper_moves(&s, &evaluate_zones(&s, &settings), &settings).is_empty());
    }

    fn eval(error: f64) -> ZoneEval {
        ZoneEval {
            number: 3,
            name: "Study".into(),
            active: true,
            target: Some(24.0),
            error: Some(error),
        }
    }

    #[test]
    fn near_target_the_compressor_idles() {
        // 12:52 on Nov 2: Study 0.6° over, return air 23.6°. It used to get 23°, a degree
        // of work on a Low fan; now the setpoint sits at the reading, rounded up.
        let ac = AcReading {
            temperature: Some(23.6),
            ..cool()
        };
        assert_eq!(setpoint_for(&ac, &eval(0.6), &live()), 24.0);
        // Past `ease_within` it works again, by the whole gap.
        assert_eq!(setpoint_for(&ac, &eval(1.4), &live()), 22.0);
        // 0 turns easing off.
        let off = Settings {
            ease_within: 0.0,
            ..live()
        };
        assert_eq!(setpoint_for(&ac, &eval(0.6), &off), 23.0);
    }

    #[test]
    fn heating_eases_downwards() {
        let ac = AcReading {
            temperature: Some(21.4),
            ..ac("heat")
        };
        assert_eq!(setpoint_for(&ac, &eval(0.6), &live()), 21.0);
    }
}
