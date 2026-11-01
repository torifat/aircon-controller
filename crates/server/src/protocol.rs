//! AirTouch 5 wire protocol (console TCP port 9005).
//!
//! Every packet is `55 55 55 AA | address (2) | message id | type | length (2) | data | CRC16`,
//! big endian, with a Modbus CRC over everything after the header. Control/status messages
//! (type `0xC0`) go to address `0x80B0`, extended messages (type `0x1F`) to `0x90B0`.
//! Layouts follow Polyaire's protocol document and the airtouch5py library, and the tests
//! below use bytes captured from a real console (firmware 1.3.3).

const HEADER: [u8; 4] = [0x55, 0x55, 0x55, 0xAA];
const ADDRESS_CONTROL: u16 = 0x80B0;
const ADDRESS_EXTENDED: u16 = 0x90B0;
const TYPE_CONTROL: u8 = 0xC0;
const TYPE_EXTENDED: u8 = 0x1F;
/// Header, address, id, type, length and CRC: a packet with no data.
const MIN_PACKET: usize = 12;

const SUB_ZONE_CONTROL: u8 = 0x20;
const SUB_ZONE_STATUS: u8 = 0x21;
const SUB_AC_CONTROL: u8 = 0x22;
const SUB_AC_STATUS: u8 = 0x23;
const EXT_AC_ABILITY: u16 = 0xFF11;
const EXT_ZONE_NAME: u16 = 0xFF13;
const EXT_CONSOLE_VERSION: u16 = 0xFF30;

/// Requests and commands we send.
#[derive(Debug, Clone, PartialEq)]
pub enum Request {
    ZoneStatus,
    AcStatus,
    AcAbility,
    ZoneNames,
    ConsoleVersion,
    ZoneControl(Vec<ZoneControl>),
    AcControl(Vec<AcControl>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoneControl {
    pub zone: u8,
    pub power: Option<bool>,
    pub value: Option<ZoneValue>,
}

/// What a zone command sets. A setpoint puts the zone in temperature control, an open
/// percentage in percentage control.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZoneValue {
    Setpoint(f64),
    Open(u8),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AcControl {
    pub ac: u8,
    pub power: Option<bool>,
    pub fan: Option<FanSpeed>,
    pub setpoint: Option<f64>,
}

/// Messages we understand from the console. Anything else decodes to `Other`.
#[derive(Debug, Clone, PartialEq)]
pub enum Message {
    ZoneStatus(Vec<ZoneStatus>),
    AcStatus(Vec<AcStatus>),
    AcAbility(Vec<AcAbility>),
    ZoneNames(Vec<(u8, String)>),
    ConsoleVersion { has_update: bool, version: String },
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZonePower {
    Off,
    On,
    Turbo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ZoneStatus {
    pub zone: u8,
    pub power: ZonePower,
    pub percentage_control: bool,
    /// Damper opening, 0-100.
    pub open: u8,
    pub setpoint: Option<f64>,
    pub has_sensor: bool,
    pub temperature: Option<f64>,
    pub spill: bool,
    pub low_battery: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcPower {
    Off,
    On,
    AwayOff,
    AwayOn,
    Sleep,
    Unknown(u8),
}

impl AcPower {
    pub fn is_on(self) -> bool {
        matches!(self, Self::On | Self::AwayOn | Self::Sleep)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcMode {
    Auto,
    Heat,
    Dry,
    Fan,
    Cool,
    AutoHeat,
    AutoCool,
    Unknown(u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FanSpeed {
    Auto,
    Quiet,
    Low,
    Medium,
    High,
    Powerful,
    Turbo,
    /// Intelligent auto and its sub-levels (0b1000..=0b1110).
    IntelligentAuto,
    Unknown(u8),
}

impl FanSpeed {
    fn from_bits(v: u8) -> Self {
        match v {
            0 => Self::Auto,
            1 => Self::Quiet,
            2 => Self::Low,
            3 => Self::Medium,
            4 => Self::High,
            5 => Self::Powerful,
            6 => Self::Turbo,
            8..=14 => Self::IntelligentAuto,
            other => Self::Unknown(other),
        }
    }

    fn to_bits(self) -> u8 {
        match self {
            Self::Auto => 0,
            Self::Quiet => 1,
            Self::Low => 2,
            Self::Medium => 3,
            Self::High => 4,
            Self::Powerful => 5,
            Self::Turbo => 6,
            Self::IntelligentAuto => 8,
            Self::Unknown(v) => v,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AcStatus {
    pub ac: u8,
    pub power: AcPower,
    pub mode: AcMode,
    pub fan: FanSpeed,
    pub setpoint: Option<f64>,
    pub temperature: Option<f64>,
    pub spill: bool,
    pub error_code: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AcAbility {
    pub ac: u8,
    pub name: String,
    pub start_zone: u8,
    pub zone_count: u8,
    pub fan_auto: bool,
    pub fan_low: bool,
    pub fan_medium: bool,
    pub fan_high: bool,
    pub cool_min: u8,
    pub cool_max: u8,
    pub heat_min: u8,
    pub heat_max: u8,
}

pub fn crc16_modbus(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= u16::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xA001
            } else {
                crc >> 1
            };
        }
    }
    crc
}

/// Setpoints travel as `(°C * 10) - 100` in one byte.
fn setpoint_byte(c: f64) -> u8 {
    (c * 10.0 - 100.0).round().clamp(0.0, 254.0) as u8
}

fn setpoint_from_byte(b: u8) -> Option<f64> {
    (b != 0xFF).then(|| (f64::from(b) + 100.0) / 10.0)
}

/// 11-bit temperatures: `(value - 500) / 10`, anything above 2000 means no reading.
fn temperature(raw: u16) -> Option<f64> {
    (raw <= 2000).then(|| (f64::from(raw) - 500.0) / 10.0)
}

pub fn encode(message_id: u8, request: &Request) -> Vec<u8> {
    let (address, kind, data) = match request {
        Request::ZoneStatus => (
            ADDRESS_CONTROL,
            TYPE_CONTROL,
            control_header(SUB_ZONE_STATUS, 0, 0),
        ),
        Request::AcStatus => (
            ADDRESS_CONTROL,
            TYPE_CONTROL,
            control_header(SUB_AC_STATUS, 0, 0),
        ),
        Request::AcAbility => (
            ADDRESS_EXTENDED,
            TYPE_EXTENDED,
            EXT_AC_ABILITY.to_be_bytes().to_vec(),
        ),
        Request::ZoneNames => (
            ADDRESS_EXTENDED,
            TYPE_EXTENDED,
            EXT_ZONE_NAME.to_be_bytes().to_vec(),
        ),
        Request::ConsoleVersion => (
            ADDRESS_EXTENDED,
            TYPE_EXTENDED,
            EXT_CONSOLE_VERSION.to_be_bytes().to_vec(),
        ),
        Request::ZoneControl(zones) => {
            let mut d = control_header(SUB_ZONE_CONTROL, 4, zones.len());
            for z in zones {
                let power = match z.power {
                    None => 0b000,
                    Some(false) => 0b010,
                    Some(true) => 0b011,
                };
                let (value_kind, value) = match z.value {
                    Some(ZoneValue::Setpoint(c)) => (0b101, setpoint_byte(c)),
                    Some(ZoneValue::Open(p)) => (0b100, p.min(100)),
                    None => (0b000, 0xFF),
                };
                d.extend([z.zone & 0x3F, (value_kind << 5) | power, value, 0]);
            }
            (ADDRESS_CONTROL, TYPE_CONTROL, d)
        }
        Request::AcControl(acs) => {
            let mut d = control_header(SUB_AC_CONTROL, 4, acs.len());
            for a in acs {
                let power = match a.power {
                    None => 0b0000,
                    Some(false) => 0b0010,
                    Some(true) => 0b0011,
                };
                let mode = 0b1111; // keep: the user owns the mode
                let fan = a.fan.map_or(0b1111, FanSpeed::to_bits);
                let (control, value) = match a.setpoint {
                    Some(c) => (0x40, setpoint_byte(c)),
                    None => (0x00, 0xFF),
                };
                d.extend([
                    (power << 4) | (a.ac & 0x0F),
                    (mode << 4) | fan,
                    control,
                    value,
                ]);
            }
            (ADDRESS_CONTROL, TYPE_CONTROL, d)
        }
    };

    let mut packet = Vec::with_capacity(MIN_PACKET + data.len());
    packet.extend(HEADER);
    packet.extend(address.to_be_bytes());
    packet.push(message_id);
    packet.push(kind);
    packet.extend((data.len() as u16).to_be_bytes());
    packet.extend(&data);
    let crc = crc16_modbus(&packet[HEADER.len()..]);
    packet.extend(crc.to_be_bytes());
    packet
}

fn control_header(sub: u8, repeat_len: u16, count: usize) -> Vec<u8> {
    let mut d = vec![sub, 0, 0, 0];
    d.extend(repeat_len.to_be_bytes());
    d.extend((count as u16).to_be_bytes());
    d
}

/// Reassembles packets from the TCP stream.
#[derive(Default)]
pub struct Decoder {
    buf: Vec<u8>,
}

impl Decoder {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Message> {
        self.buf.extend_from_slice(bytes);
        let mut out = Vec::new();
        loop {
            // Skip to the next header. The console also sends `55 55 55 AB` preambles,
            // which this drops along with any other noise.
            match self.buf.windows(HEADER.len()).position(|w| w == HEADER) {
                Some(0) => {}
                Some(start) => {
                    self.buf.drain(..start);
                }
                None => {
                    let keep = self.buf.len().min(HEADER.len() - 1);
                    self.buf.drain(..self.buf.len() - keep);
                    return out;
                }
            }
            if self.buf.len() < MIN_PACKET {
                return out;
            }
            let len = usize::from(u16::from_be_bytes([self.buf[8], self.buf[9]]));
            let total = MIN_PACKET + len;
            if self.buf.len() < total {
                return out;
            }
            let packet: Vec<u8> = self.buf.drain(..total).collect();
            let crc = u16::from_be_bytes([packet[total - 2], packet[total - 1]]);
            if crc16_modbus(&packet[HEADER.len()..total - 2]) != crc {
                tracing::debug!("dropping packet with bad CRC");
                continue;
            }
            match decode(packet[7], &packet[10..10 + len]) {
                Ok(m) => out.push(m),
                Err(e) => tracing::debug!("undecodable packet: {e}"),
            }
        }
    }
}

fn decode(kind: u8, data: &[u8]) -> Result<Message, String> {
    match kind {
        TYPE_CONTROL => decode_control(data),
        TYPE_EXTENDED => decode_extended(data),
        _ => Ok(Message::Other),
    }
}

fn be16(d: &[u8], at: usize) -> Result<u16, String> {
    d.get(at..at + 2)
        .map(|b| u16::from_be_bytes([b[0], b[1]]))
        .ok_or_else(|| "short packet".to_owned())
}

fn decode_control(d: &[u8]) -> Result<Message, String> {
    let sub = *d.first().ok_or("empty control message")?;
    let normal_len = usize::from(be16(d, 2)?);
    let repeat_len = usize::from(be16(d, 4)?);
    let count = usize::from(be16(d, 6)?);
    let body = d.get(8 + normal_len..).ok_or("short control message")?;
    if body.len() != repeat_len * count {
        return Err(format!(
            "expected {} repeat bytes, got {}",
            repeat_len * count,
            body.len()
        ));
    }
    let items = || body.chunks_exact(repeat_len.max(1)).take(count);
    match sub {
        SUB_ZONE_STATUS if count == 0 || repeat_len >= 8 => {
            Ok(Message::ZoneStatus(items().map(zone_status).collect()))
        }
        SUB_AC_STATUS if count == 0 || repeat_len >= 10 => {
            Ok(Message::AcStatus(items().map(ac_status).collect()))
        }
        SUB_ZONE_STATUS | SUB_AC_STATUS => Err(format!("repeat length {repeat_len} too short")),
        _ => Ok(Message::Other), // echoes of control messages
    }
}

fn zone_status(b: &[u8]) -> ZoneStatus {
    ZoneStatus {
        power: match b[0] >> 6 {
            0b01 => ZonePower::On,
            0b11 => ZonePower::Turbo,
            _ => ZonePower::Off,
        },
        zone: b[0] & 0x3F,
        percentage_control: b[1] & 0x80 == 0,
        open: b[1] & 0x7F,
        setpoint: setpoint_from_byte(b[2]),
        has_sensor: b[3] & 0x80 != 0,
        temperature: temperature((u16::from(b[4] & 0x07) << 8) | u16::from(b[5])),
        spill: b[6] & 0x02 != 0,
        low_battery: b[6] & 0x01 != 0,
    }
}

fn ac_status(b: &[u8]) -> AcStatus {
    AcStatus {
        power: match b[0] >> 4 {
            0 => AcPower::Off,
            1 => AcPower::On,
            2 => AcPower::AwayOff,
            3 => AcPower::AwayOn,
            5 => AcPower::Sleep,
            other => AcPower::Unknown(other),
        },
        ac: b[0] & 0x0F,
        mode: match b[1] >> 4 {
            0 => AcMode::Auto,
            1 => AcMode::Heat,
            2 => AcMode::Dry,
            3 => AcMode::Fan,
            4 => AcMode::Cool,
            8 => AcMode::AutoHeat,
            9 => AcMode::AutoCool,
            other => AcMode::Unknown(other),
        },
        fan: FanSpeed::from_bits(b[1] & 0x0F),
        // Unlike zones, 0xFF isn't documented as "invalid" here, but treat it the same.
        setpoint: setpoint_from_byte(b[2]),
        // Byte 4 also carries turbo/bypass/timer bits; on firmware 1.3.3 they don't match
        // the documented meanings, so only spill is read.
        spill: b[3] & 0x02 != 0,
        temperature: temperature((u16::from(b[4] & 0x07) << 8) | u16::from(b[5])),
        error_code: u16::from_be_bytes([b[6], b[7]]),
    }
}

fn decode_extended(d: &[u8]) -> Result<Message, String> {
    let sub = be16(d, 0)?;
    let mut rest = &d[2..];
    match sub {
        EXT_AC_ABILITY => {
            let mut acs = Vec::new();
            while rest.len() > 2 {
                let len = usize::from(rest[1]);
                let b = rest
                    .get(..2 + len)
                    .filter(|b| b.len() >= 26)
                    .ok_or("short AC ability")?;
                let name = String::from_utf8_lossy(&b[2..18]);
                acs.push(AcAbility {
                    ac: b[0],
                    name: name.split('\0').next().unwrap_or_default().to_owned(),
                    start_zone: b[18],
                    zone_count: b[19],
                    fan_auto: b[21] & 0x01 != 0,
                    fan_low: b[21] & 0x04 != 0,
                    fan_medium: b[21] & 0x08 != 0,
                    fan_high: b[21] & 0x10 != 0,
                    cool_min: b[22],
                    cool_max: b[23],
                    heat_min: b[24],
                    heat_max: b[25],
                });
                rest = &rest[2 + len..];
            }
            Ok(Message::AcAbility(acs))
        }
        EXT_ZONE_NAME => {
            let mut names = Vec::new();
            while rest.len() > 2 {
                let len = usize::from(rest[1]);
                let name = rest.get(2..2 + len).ok_or("short zone name")?;
                names.push((rest[0], String::from_utf8_lossy(name).into_owned()));
                rest = &rest[2 + len..];
            }
            Ok(Message::ZoneNames(names))
        }
        EXT_CONSOLE_VERSION if rest.len() >= 2 => {
            let len = usize::from(rest[1]);
            let version = rest.get(2..2 + len).ok_or("short console version")?;
            Ok(Message::ConsoleVersion {
                has_update: rest[0] != 0,
                version: String::from_utf8_lossy(version).into_owned(),
            })
        }
        _ => Ok(Message::Other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        (0..s.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
            .collect()
    }

    fn decode_one(s: &str) -> Message {
        let mut msgs = Decoder::default().push(&hex(s));
        assert_eq!(msgs.len(), 1, "{msgs:?}");
        msgs.remove(0)
    }

    // Expected bytes come from airtouch5py's encoder.
    #[test]
    fn encodes_requests() {
        let cases = [
            (
                Request::ZoneStatus,
                "555555aa80b001c000082100000000000000a431",
            ),
            (
                Request::AcStatus,
                "555555aa80b001c0000823000000000000007db0",
            ),
            (Request::AcAbility, "555555aa90b0011f0002ff11834c"),
            (Request::ZoneNames, "555555aa90b0011f0002ff1342cd"),
            (Request::ConsoleVersion, "555555aa90b0011f0002ff309b8c"),
        ];
        for (req, expected) in cases {
            assert_eq!(encode(1, &req), hex(expected), "{req:?}");
        }
    }

    #[test]
    fn encodes_ac_control() {
        let fan_and_setpoint = AcControl {
            ac: 0,
            power: None,
            fan: Some(FanSpeed::High),
            setpoint: Some(21.0),
        };
        assert_eq!(
            encode(10, &Request::AcControl(vec![fan_and_setpoint])),
            hex("555555aa80b00ac0000c220000000004000100f4406e768b")
        );
        let off = AcControl {
            ac: 0,
            power: Some(false),
            fan: None,
            setpoint: None,
        };
        assert_eq!(
            encode(10, &Request::AcControl(vec![off])),
            hex("555555aa80b00ac0000c220000000004000120ff00ff1801")
        );
    }

    #[test]
    fn encodes_zone_control() {
        let off = ZoneControl {
            zone: 1,
            power: Some(false),
            value: None,
        };
        assert_eq!(
            encode(10, &Request::ZoneControl(vec![off])),
            hex("555555aa80b00ac0000c20000000000400010102ff00a362")
        );
        let on = ZoneControl {
            zone: 0,
            power: Some(true),
            value: Some(ZoneValue::Setpoint(23.5)),
        };
        assert_eq!(
            encode(10, &Request::ZoneControl(vec![on])),
            hex("555555aa80b00ac0000c200000000004000100a38700bd10")
        );
        let open = ZoneControl {
            zone: 2,
            power: None,
            value: Some(ZoneValue::Open(100)),
        };
        let packet = encode(10, &Request::ZoneControl(vec![open]));
        assert_eq!(
            &packet[packet.len() - 6..packet.len() - 2],
            [0x02, 0x80, 100, 0]
        );
    }

    // Captured from the console, preamble included.
    #[test]
    fn decodes_zone_status() {
        let Message::ZoneStatus(zones) = decode_one(
            "555555ab000000440044555555aab08001c00038210000000008000600808c8002e700000194828002\
             ee000002328c8002ec000043648ce402f1000004808c8002ec000005948c8002eb00009cbb",
        ) else {
            panic!("not zone status");
        };
        assert_eq!(zones.len(), 6);
        assert_eq!(
            zones[1],
            ZoneStatus {
                zone: 1,
                power: ZonePower::Off,
                percentage_control: false,
                open: 20,
                setpoint: Some(23.0),
                has_sensor: true,
                temperature: Some(25.0),
                spill: false,
                low_battery: false,
            }
        );
        let study = &zones[3];
        assert_eq!(study.power, ZonePower::On);
        assert!(study.percentage_control);
        assert_eq!(study.open, 100);
        assert_eq!(study.temperature, Some(25.3));
    }

    #[test]
    fn decodes_ac_status() {
        let Message::AcStatus(acs) =
            decode_one("555555aab08001c0001623000000000e000110928cc93ae400008000a2e400009879")
        else {
            panic!("not AC status");
        };
        assert_eq!(
            acs,
            vec![AcStatus {
                ac: 0,
                power: AcPower::On,
                mode: AcMode::AutoCool,
                fan: FanSpeed::Low,
                setpoint: Some(24.0),
                temperature: Some(24.0),
                spill: false,
                error_code: 0,
            }]
        );
    }

    #[test]
    fn decodes_ac_ability() {
        let Message::AcAbility(acs) = decode_one(
            "555555ab000000280028555555aab090011f001cff1100184461696b696e00000000000000000000\
             00061f1d1219101a9e66",
        ) else {
            panic!("not AC ability");
        };
        let a = &acs[0];
        assert_eq!(a.name, "Daikin");
        assert_eq!((a.start_zone, a.zone_count), (0, 6));
        assert!(a.fan_auto && a.fan_low && a.fan_medium && a.fan_high);
        assert_eq!(
            (a.cool_min, a.cool_max, a.heat_min, a.heat_max),
            (18, 25, 16, 26)
        );
    }

    #[test]
    fn decodes_zone_names_and_version() {
        let Message::ZoneNames(names) = decode_one(
            // Captured from the console, with two zone names replaced (lengths and CRC redone).
            "555555ab000000500050555555aab090011f0044ff13000a4d6173746572204265640109426564726f\
             6f6d20320209426564726f6f6d203303055374756479040a477565737420526f6f6d050b4c6976696e\
             672041726561c37c",
        ) else {
            panic!("not zone names");
        };
        let names: Vec<_> = names.iter().map(|(_, n)| n.as_str()).collect();
        assert_eq!(
            names,
            [
                "Master Bed",
                "Bedroom 2",
                "Bedroom 3",
                "Study",
                "Guest Room",
                "Living Area"
            ]
        );

        assert_eq!(
            decode_one("555555ab000000150015555555aab090011f0009ff300005312e332e33fdf8"),
            Message::ConsoleVersion {
                has_update: false,
                version: "1.3.3".into()
            }
        );
    }

    #[test]
    fn reassembles_split_packets_and_skips_bad_crc() {
        let good = hex("555555aab08001c0001623000000000e000110928cc93ae400008000a2e400009879");
        let mut bad = good.clone();
        *bad.last_mut().unwrap() ^= 0xFF;
        let mut d = Decoder::default();
        assert!(d.push(&bad).is_empty());
        assert!(d.push(&good[..15]).is_empty());
        assert_eq!(d.push(&good[15..]).len(), 1);
    }
}
