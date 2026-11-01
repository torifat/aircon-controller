//! The control loop's brain: one pure function from (snapshot, settings, memory) to a decision.
//!
//! The AirTouch closes dampers as rooms reach target but the Daikin keeps blowing at full
//! tilt, so the surplus lands in the spill zone. Instead we throttle the AC itself, driven
//! by whichever active zone is furthest from its target:
//!
//! - fan steps Low / Medium / High with the size of that gap;
//! - the AC setpoint is pulled away from the AC's own reading by the gap, so the inverter
//!   compressor works hard when rooms are far off and idles near target;
//! - when every active zone reaches target the AC is turned off ("parked"), and turned back
//!   on once any zone drifts `deadband` degrees past its target;
//! - min on/off times protect the compressor;
//! - a fan/setpoint/mode change made outside the controller pauses it for a while.
//!
//! With a single room on, the AirTouch closing that room's damper near target only pushes
//! air into the spill zone, so `damper()` opens it fully and the AC alone holds it.

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
pub fn fan_for(gap: f64, current: Fan, s: &Settings, available: &[Fan]) -> Fan {
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
pub fn setpoint_for(ac: &AcReading, worst: &ZoneEval, s: &Settings) -> f64 {
    let reference = ac.temperature.or(worst.target).unwrap_or(24.0);
    let offset = (s.setpoint_gain * worst.error.unwrap_or(0.0).max(0.0)).min(s.max_setpoint_offset);
    let raw = reference - ac.direction.sign() * offset;
    let stepped = (raw / s.setpoint_step).round() * s.setpoint_step;
    ac.limits.clamp(stepped, ac.direction)
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

/// A change to a room's damper for the lone-room rule.
#[derive(Debug, Clone, PartialEq)]
pub enum Damper {
    /// Switch the only room on to percentage control at 100%, keeping its target here.
    Open { zone: u8, name: String, target: f64 },
    /// Hand a room we opened back to the AirTouch's temperature control at its target.
    Restore {
        zone: u8,
        name: String,
        target: f64,
        reason: String,
    },
    /// The room we opened was switched back (in the app) or is gone; stop tracking it.
    Release,
}

/// Open the only room on fully, and hand it back once it isn't the only one.
///
/// `rt` is the runtime after this tick's `decide`, so a pause it started counts. Opening
/// waits for the controller to be live; handing back doesn't, so turning the controller
/// off or to dry run never leaves a room stuck open.
pub fn damper(snap: &Snapshot, s: &Settings, rt: &Runtime, now: f64) -> Option<Damper> {
    let live = s.enabled && !s.dry_run && rt.paused_until.is_none_or(|t| now >= t);
    let counted = |z: &ZoneReading| s.zones.as_ref().is_none_or(|zs| zs.contains(&z.number));
    let on: Vec<&ZoneReading> = snap.zones.iter().filter(|z| z.on).collect();

    if let Some(n) = rt.opened {
        let Some(z) = snap.zones.iter().find(|z| z.number == n) else {
            return Some(Damper::Release);
        };
        if z.control == ZoneControl::Temperature {
            return Some(Damper::Release);
        }
        let reason = if !z.on {
            format!("{} turned off", z.name)
        } else if on.len() > 1 {
            let others: Vec<&str> = on
                .iter()
                .filter(|o| o.number != n)
                .map(|o| o.name.as_str())
                .collect();
            format!(
                "{} on too; AirTouch balances the rooms again",
                others.join(", ")
            )
        } else if !s.enabled {
            "Controller is off".to_owned()
        } else if s.dry_run {
            "Dry run".to_owned()
        } else if !counted(z) {
            format!("{} no longer counts", z.name)
        } else if !live {
            "Controller paused".to_owned()
        } else {
            return None;
        };
        let Some(target) = s.targets.get(&n).copied().or(z.setpoint) else {
            return Some(Damper::Release);
        };
        return Some(Damper::Restore {
            zone: n,
            name: z.name.clone(),
            target,
            reason,
        });
    }

    let [z] = on.as_slice() else {
        return None;
    };
    if !live || !counted(z) || z.control != ZoneControl::Temperature || !z.has_sensor {
        return None;
    }
    Some(Damper::Open {
        zone: z.number,
        name: z.name.clone(),
        target: z.setpoint?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{SetpointLimits, ZoneReading};

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
            (0.9, Fan::Medium, Fan::Medium),
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

    fn opened(n: u8) -> Runtime {
        Runtime {
            opened: Some(n),
            ..Runtime::default()
        }
    }

    fn off(z: ZoneReading) -> ZoneReading {
        ZoneReading { on: false, ..z }
    }

    fn held_open(z: ZoneReading) -> ZoneReading {
        ZoneReading {
            control: ZoneControl::Percentage,
            damper: 1.0,
            ..z
        }
    }

    #[test]
    fn lone_room_opens_fully() {
        let s = snap(
            cool(),
            vec![named("Mouni", zone(1, 24.3)), off(zone(2, 23.5))],
        );
        let open = Damper::Open {
            zone: 1,
            name: "Mouni".into(),
            target: 24.0,
        };
        assert_eq!(damper(&s, &live(), &Runtime::default(), NOW), Some(open));
        // Not in dry run, while paused, or with the controller off.
        assert_eq!(
            damper(&s, &Settings::default(), &Runtime::default(), NOW),
            None
        );
        let paused = Runtime {
            paused_until: Some(NOW + 60.0),
            ..Runtime::default()
        };
        assert_eq!(damper(&s, &live(), &paused, NOW), None);
        let disabled = Settings {
            enabled: false,
            ..live()
        };
        assert_eq!(damper(&s, &disabled, &Runtime::default(), NOW), None);
    }

    #[test]
    fn several_rooms_stay_with_the_airtouch() {
        let s = snap(cool(), vec![zone(1, 24.3), zone(2, 25.0)]);
        assert_eq!(damper(&s, &live(), &Runtime::default(), NOW), None);
    }

    #[test]
    fn lone_room_needs_temperature_control_and_to_count() {
        let pct = ZoneReading {
            control: ZoneControl::Percentage,
            ..zone(1, 24.3)
        };
        let s = snap(cool(), vec![pct]);
        assert_eq!(damper(&s, &live(), &Runtime::default(), NOW), None);
        let s = snap(cool(), vec![zone(1, 24.3)]);
        let elsewhere = Settings {
            zones: Some(vec![2]),
            ..live()
        };
        assert_eq!(damper(&s, &elsewhere, &Runtime::default(), NOW), None);
    }

    #[test]
    fn opened_room_stays_open_while_alone() {
        let s = snap(cool(), vec![held_open(zone(1, 24.3)), off(zone(2, 23.5))]);
        assert_eq!(damper(&s, &live(), &opened(1), NOW), None);
    }

    #[test]
    fn second_room_hands_back() {
        let mut settings = live();
        settings.targets.insert(1, 23.5);
        let s = snap(
            cool(),
            vec![
                named("Mouni", held_open(zone(1, 24.3))),
                named("Study", zone(2, 25.0)),
            ],
        );
        assert_eq!(
            damper(&s, &settings, &opened(1), NOW),
            Some(Damper::Restore {
                zone: 1,
                name: "Mouni".into(),
                target: 23.5,
                reason: "Study on too; AirTouch balances the rooms again".into(),
            })
        );
    }

    #[test]
    fn hands_back_when_off_or_not_live() {
        let s = snap(cool(), vec![named("Mouni", off(held_open(zone(1, 24.3))))]);
        let Some(Damper::Restore { reason, .. }) = damper(&s, &live(), &opened(1), NOW) else {
            panic!("expected a restore");
        };
        assert_eq!(reason, "Mouni turned off");

        let s = snap(cool(), vec![held_open(zone(1, 24.3))]);
        let Some(Damper::Restore { reason, .. }) =
            damper(&s, &Settings::default(), &opened(1), NOW)
        else {
            panic!("expected a restore");
        };
        assert_eq!(reason, "Dry run");
    }

    #[test]
    fn room_switched_back_in_the_app_is_released() {
        let s = snap(cool(), vec![zone(1, 24.3)]);
        assert_eq!(damper(&s, &live(), &opened(1), NOW), Some(Damper::Release));
        let s = snap(cool(), vec![zone(2, 24.3)]);
        assert_eq!(damper(&s, &live(), &opened(1), NOW), Some(Damper::Release));
    }
}
