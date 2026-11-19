use std::io;

use super::models::{Layout, Position};
use super::report::{Packet, Reply, packet};
use crate::witmod_sdk::keymap::KEY_COLUMNS;

const COMMAND: u8 = 0x29;
const RESPONSE: u8 = 0x21;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Thresholds {
    Shared(u8),
    Separate { press: u8, release: u8 },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Tuning {
    pub actuation: Thresholds,
    pub rapid: Option<Thresholds>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Control {
    AntiShake,
    ReturnRate,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Readback {
    pub mode: u8,
    pub press: u8,
    pub release: u8,
    pub rapid_press: u8,
    pub rapid_release: u8,
}

impl Control {
    fn selector(self) -> u8 {
        match self {
            Self::AntiShake => 10,
            Self::ReturnRate => 9,
        }
    }

    pub fn request(self, value: Option<u8>) -> Packet {
        let mut request = command(self.selector());
        if let Some(value) = value {
            request[7..9].copy_from_slice(&[1, value]);
        }
        request
    }

    pub(crate) fn reply(self) -> Reply {
        reply(self.selector(), 8)
    }
}

pub fn apply(layout: &Layout, keys: &[Position], tuning: Tuning) -> io::Result<Packet> {
    if !layout.magnetic() || keys.is_empty() || keys.iter().any(|key| layout.key(*key).is_none()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "select at least one populated position on a magnetic layout",
        ));
    }
    let separate = matches!(tuning.actuation, Thresholds::Separate { .. });
    if tuning
        .rapid
        .is_some_and(|rapid| matches!(rapid, Thresholds::Separate { .. }) != separate)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "use the same shared or separate mode for actuation and rapid trigger",
        ));
    }
    let mode = u8::from(separate) | if tuning.rapid.is_some() { 0x0c } else { 0 };
    let (press, release) = values(tuning.actuation);
    let (rapid_press, rapid_release) = tuning.rapid.map(values).unwrap_or_default();
    if press == 0
        || release == 0
        || tuning.rapid.is_some() && (rapid_press == 0 || rapid_release == 0)
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "active thresholds must be positive",
        ));
    }
    let mut report = command(mode);
    set_masks(&mut report, keys);
    report[7 + KEY_COLUMNS..][..4].copy_from_slice(&[press, release, rapid_press, rapid_release]);
    Ok(report)
}

pub(crate) fn stop_travel() -> Packet {
    command(3)
}

pub(crate) fn decode_travel(report: &[u8]) -> Option<(Position, u8)> {
    if report.get(1) != Some(&RESPONSE) || report.get(6) != Some(&1) {
        return None;
    }
    let &[row, column, travel] = report.get(7..10)? else {
        return None;
    };
    Some((Position::new(row, column)?, travel))
}

pub fn key_request(key: Position) -> Packet {
    let mut report = command(5);
    report[7..9].copy_from_slice(&[key.row(), key.column()]);
    report
}

pub(crate) fn disable_rapid_report(key: Position, current: Readback) -> io::Result<Option<Packet>> {
    match current.mode {
        0 => return Ok(None),
        1 | 0x0c | 0x0d => {}
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unrecognized trigger mode",
            ));
        }
    };
    if current.press == 0 || current.release == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid saved actuation threshold",
        ));
    }
    if current.press != current.release {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the vendor off mode requires matching press and release thresholds",
        ));
    }
    let mut report = command(0);
    set_masks(&mut report, &[key]);
    report[29..33].copy_from_slice(&[
        current.press,
        current.release,
        current.rapid_press,
        current.rapid_release,
    ]);
    Ok(Some(report))
}

pub(crate) fn key_reply() -> Reply {
    reply(5, 12)
}

pub fn decode_key(report: &[u8]) -> Option<Readback> {
    if report.get(1) != Some(&RESPONSE) || report.get(6) != Some(&5) {
        return None;
    }
    let &[mode, press, release, rapid_press, rapid_release] = report.get(7..12)? else {
        return None;
    };
    Some(Readback {
        mode,
        press,
        release,
        rapid_press,
        rapid_release,
    })
}

pub fn sensitivity_request() -> Packet {
    command(4)
}

pub(crate) fn sensitivity_reply() -> Reply {
    reply(4, 8)
}

pub fn defaults_request() -> Packet {
    command(6)
}

pub fn calibration_selection(keys: &[Position]) -> Packet {
    let mut report = command(2);
    set_masks(&mut report, keys);
    report
}

pub fn calibration_all(enabled: bool) -> Packet {
    let mut report = command(8);
    report[7] = u8::from(!enabled);
    report
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CalibrationStatus {
    Single(u8),
    All {
        status: u8,
        completed: [u8; KEY_COLUMNS],
    },
}

pub fn decode_calibration(report: &[u8]) -> Option<CalibrationStatus> {
    if report.get(1) != Some(&RESPONSE) {
        return None;
    }
    let status = *report.get(7)?;
    match report.get(6)? {
        7 => Some(CalibrationStatus::Single(status)),
        8 => {
            let mut completed = [0; KEY_COLUMNS];
            if status <= 1 {
                completed.copy_from_slice(report.get(8..8 + KEY_COLUMNS)?);
            }
            Some(CalibrationStatus::All { status, completed })
        }
        _ => None,
    }
}

pub fn calibration_start(key: Position) -> Packet {
    let mut report = command(7);
    report[7..9].copy_from_slice(&[key.row(), key.column()]);
    report
}

fn set_masks(report: &mut Packet, keys: &[Position]) {
    for key in keys {
        report[7 + usize::from(key.column())] |= 1 << key.row();
    }
}

fn command(selector: u8) -> Packet {
    let mut report = packet(COMMAND, [0, 0, 0, 0x18]);
    report[6] = selector;
    report
}

fn reply(selector: u8, min_len: usize) -> Reply {
    Reply {
        command: RESPONSE,
        selector: Some((6, selector)),
        min_len,
    }
}

fn values(thresholds: Thresholds) -> (u8, u8) {
    match thresholds {
        Thresholds::Shared(value) => (value, value),
        Thresholds::Separate { press, release } => (press, release),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyboard::models::find;
    use crate::witmod_sdk::hid::{ReportId, frame_write_report};

    #[test]
    fn live_travel_checks_the_selector_and_matrix_position() {
        let mut report = [1, 0x21, 0, 0, 0, 0, 1, 2, 2, 41];
        assert_eq!(
            decode_travel(&report),
            Some((Position::new(2, 2).unwrap(), 41))
        );
        assert!(decode_travel(&report[..9]).is_none());
        report[8] = 22;
        assert!(decode_travel(&report).is_none());
        assert_eq!(&stop_travel()[..7], &[1, 0x29, 0, 0, 0, 0x18, 3]);
    }

    #[test]
    fn disables_rapid_for_w_without_replacing_any_threshold() {
        let key = Position::new(2, 2).unwrap();
        let current = Readback {
            mode: 1,
            press: 1,
            release: 1,
            rapid_press: 1,
            rapid_release: 1,
        };
        let report = disable_rapid_report(key, current).unwrap().unwrap();
        assert_eq!(report[6], 0);
        assert_eq!(report[9], 4);
        assert_eq!(report[7..29].iter().filter(|&&mask| mask != 0).count(), 1);
        assert_eq!(&report[29..33], &[1, 1, 1, 1]);
        assert_eq!(
            disable_rapid_report(key, Readback { mode: 0, ..current }).unwrap(),
            None
        );
        assert!(
            disable_rapid_report(
                key,
                Readback {
                    mode: 0x0d,
                    release: 7,
                    ..current
                }
            )
            .is_err()
        );
    }

    #[test]
    fn packs_apply_in_the_vendor_column_order() {
        let layout = find("7153_AUSHERGB").unwrap();
        let keys = [
            Position::new(1, 0).unwrap(),
            Position::new(2, 0).unwrap(),
            Position::new(5, 12).unwrap(),
        ];
        let tuning = Tuning {
            actuation: Thresholds::Separate {
                press: 15,
                release: 16,
            },
            rapid: Some(Thresholds::Separate {
                press: 2,
                release: 3,
            }),
        };
        let app = apply(layout, &keys, tuning).unwrap();
        let mut expected = [0; 64];
        expected[..8].copy_from_slice(&[1, 0x29, 0, 0, 0, 0x18, 0x0d, 0b110]);
        expected[19] = 0b100000;
        expected[29..33].copy_from_slice(&[15, 16, 2, 3]);
        assert_eq!(app, expected);
        expected[..2].copy_from_slice(&[6, 0x21]);
        assert_eq!(
            frame_write_report(app, ReportId::Collection06).unwrap(),
            expected
        );
        assert!(apply(layout, &[Position::new(0, 0).unwrap()], tuning).is_err());
    }

    #[test]
    fn key_reply_is_bounded_and_control_queries_have_verified_flags() {
        let reply = [1, 0x21, 0, 0, 0, 0, 5, 13, 31, 30, 2, 3];
        for prefix in 0..reply.len() {
            assert_eq!(decode_key(&reply[..prefix]), None);
        }
        assert_eq!(decode_key(&reply).unwrap().rapid_release, 3);
        assert_eq!(
            &Control::AntiShake.request(None)[..9],
            &[1, 0x29, 0, 0, 0, 24, 10, 0, 0]
        );
        assert_eq!(
            &Control::ReturnRate.request(Some(4))[..9],
            &[1, 0x29, 0, 0, 0, 24, 9, 1, 4]
        );
    }
}
