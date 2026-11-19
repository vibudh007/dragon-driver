use super::report::{Packet, Reply, packet};

const COMMAND: u8 = 0x13;
const RESPONSE: u8 = 0x14;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LinkStatus {
    pub report_flag: bool,
    pub charging_state: u8,
    pub battery_level: u8,
    pub transport: u8,
    pub dongle_state: u8,
}

impl LinkStatus {
    pub(crate) fn decode(report: &[u8]) -> Option<Self> {
        if report.get(1) != Some(&1) || report.get(5) == Some(&0) {
            return None;
        }
        let &[charging_state, battery_level, transport, dongle_state] = report.get(9..13)? else {
            return None;
        };
        Some(Self {
            report_flag: report[6] == 1,
            charging_state,
            battery_level,
            transport,
            dongle_state,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Sleep {
    pub lights_seconds: u16,
    pub keyboard_seconds: u16,
    pub flags: u8,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct WinLock {
    pub windows_key: u8,
    pub alt_tab: u8,
    pub alt_f4: u8,
    pub shift_tab: u8,
}

impl Sleep {
    pub fn request(self) -> Packet {
        let mut report = command(1);
        report[9..11].copy_from_slice(&self.lights_seconds.to_be_bytes());
        report[11..13].copy_from_slice(&self.keyboard_seconds.to_be_bytes());
        report[13] = self.flags;
        report
    }

    pub fn decode(report: &[u8]) -> Option<Self> {
        if report.get(1..3) != Some(&[RESPONSE, 2]) {
            return None;
        }
        let &[light_hi, light_lo, kb_hi, kb_lo, flags] = report.get(9..14)? else {
            return None;
        };
        Some(Self {
            lights_seconds: u16::from_be_bytes([light_hi, light_lo]),
            keyboard_seconds: u16::from_be_bytes([kb_hi, kb_lo]),
            flags,
        })
    }
}

impl WinLock {
    pub fn request(self) -> Packet {
        let mut report = command(0);
        report[13..17].copy_from_slice(&[
            self.windows_key,
            self.alt_tab,
            self.shift_tab,
            self.alt_f4,
        ]);
        report
    }
}

pub fn sleep_query() -> Packet {
    command(2)
}

pub(crate) fn status_query() -> Packet {
    packet(0, [0, 0, 0, 1])
}

pub(crate) fn status_reply() -> Reply {
    Reply {
        command: 1,
        selector: None,
        min_len: 13,
    }
}

pub fn reset() -> Packet {
    let mut report = command(0);
    report[6] = 1;
    report
}

pub fn gamepad_mode(value: u8) -> Packet {
    let mut report = command(3);
    report[6] = value;
    report
}

pub(crate) fn sleep_reply() -> Reply {
    Reply {
        command: RESPONSE,
        selector: Some((2, 2)),
        min_len: 14,
    }
}

pub(crate) fn sleep_write_reply() -> Reply {
    Reply {
        command: RESPONSE,
        selector: Some((2, 0)),
        min_len: 3,
    }
}

fn command(selector: u8) -> Packet {
    packet(COMMAND, [selector, 0, 0, 1])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_vendor_status_and_connection_fields() {
        assert_eq!(status_query()[..7], [1, 0, 0, 0, 0, 1, 0]);
        let mut report = [1, 1, 0, 0, 0, 8, 1, 0, 0, 2, 100, 1, 4];
        let status = LinkStatus::decode(&report).unwrap();
        assert!(status.report_flag);
        assert_eq!(status.charging_state, 2);
        assert_eq!(status.battery_level, 100);
        assert_eq!(status.transport, 1);
        assert_eq!(status.dongle_state, 4);
        assert!(LinkStatus::decode(&report[..12]).is_none());
        report[5] = 0;
        assert!(LinkStatus::decode(&report).is_none());
    }

    #[test]
    fn sleep_timers_are_big_endian() {
        let settings = Sleep {
            lights_seconds: 600,
            keyboard_seconds: 30,
            flags: 1,
        };
        let mut request = settings.request();
        assert_eq!(
            request[..14],
            [1, 0x13, 1, 0, 0, 1, 0, 0, 0, 0x02, 0x58, 0, 30, 1]
        );
        request[1..3].copy_from_slice(&[0x14, 2]);
        assert_eq!(Sleep::decode(&request[..14]), Some(settings));
        assert_eq!(Sleep::decode(&request[..13]), None);
    }
}
