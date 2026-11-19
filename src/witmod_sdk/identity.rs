use std::io;
use std::thread;
use std::time::Duration;

use super::catalog::{MODELS, Model};
use super::hid::{ReportId, SDK_REPORT_LEN};
use super::hidraw::HidrawDevice;

const COMMAND: u8 = 0x0d;
const PAYLOAD_OFFSET: usize = 6;
const REPLY_DELAY: Duration = Duration::from_millis(250);
const MAX_ATTEMPTS: usize = 3;

#[derive(Debug, Eq, PartialEq)]
pub enum FirstReply<'a> {
    Retry,
    Fields { flags: u8, tokens: [&'a [u8]; 6] },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReplyError {
    WrongCommand,
    Truncated,
}

#[derive(Debug, Eq, PartialEq)]
pub struct Identity {
    pub fields: [Vec<u8>; 6],
    pub feature_flag: bool,
    pub build_stamp: Option<String>,
}

impl Identity {
    pub fn controller(&self) -> &[u8] {
        &self.fields[0]
    }

    pub fn layout_code(&self) -> &[u8] {
        &self.fields[4]
    }

    pub fn firmware_version(&self) -> &[u8] {
        &self.fields[5]
    }

    pub fn model(&self) -> Option<&'static Model> {
        MODELS
            .iter()
            .find(|model| model.hid_name.as_bytes() == self.layout_code())
    }
}

pub fn request(report_id: ReportId) -> [u8; SDK_REPORT_LEN] {
    let mut report = [0; SDK_REPORT_LEN];
    report[0] = report_id.byte();
    report[1] = COMMAND;
    report
}

pub fn first_reply(report: &[u8]) -> Result<FirstReply<'_>, ReplyError> {
    let report = &report[..report.len().min(SDK_REPORT_LEN)];
    if report.len() < PAYLOAD_OFFSET {
        return Err(ReplyError::Truncated);
    }
    if report[1] != COMMAND {
        return Err(ReplyError::WrongCommand);
    }
    let payload = payload(report).ok_or(ReplyError::Truncated)?;
    if payload.contains(&b':') {
        return Ok(FirstReply::Retry);
    }
    let mut tokens = [&[][..]; 6];
    for (target, token) in tokens.iter_mut().zip(
        payload
            .split(|&byte| byte == b',')
            .filter(|s| !s.is_empty()),
    ) {
        *target = token;
    }
    if tokens[0].is_empty() {
        return Err(ReplyError::Truncated);
    }
    Ok(FirstReply::Fields {
        flags: report[2],
        tokens,
    })
}

fn payload(report: &[u8]) -> Option<&[u8]> {
    report
        .get(PAYLOAD_OFFSET..report.len().min(SDK_REPORT_LEN))?
        .split(|&byte| byte == 0)
        .next()
}

pub fn second_reply(report: &[u8]) -> Option<String> {
    let payload = std::str::from_utf8(payload(report)?).ok()?;
    let mut fields = payload.split(',').filter(|field| !field.is_empty());
    let mut date = fields.next()?.split(' ').filter(|field| !field.is_empty());
    let month = match date.next()? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let day = date.next()?;
    let year = date.next()?;
    let time = fields.next()?;
    Some(format!("{year}/{month}/{day} {time}"))
}

pub(super) fn identify(device: &mut HidrawDevice, report_id: ReportId) -> io::Result<Identity> {
    exchange(device, report_id, thread::sleep)
}

trait IdentityIo {
    fn send(&mut self, report: &[u8; SDK_REPORT_LEN]) -> io::Result<usize>;
    fn receive(&mut self, report: &mut [u8; SDK_REPORT_LEN]) -> io::Result<usize>;
    fn wait_for_optional_reply(&self) -> io::Result<bool>;
}

impl IdentityIo for HidrawDevice {
    fn send(&mut self, report: &[u8; SDK_REPORT_LEN]) -> io::Result<usize> {
        self.write_framed_report(report)
    }

    fn receive(&mut self, report: &mut [u8; SDK_REPORT_LEN]) -> io::Result<usize> {
        self.read_report(report)
    }

    fn wait_for_optional_reply(&self) -> io::Result<bool> {
        self.wait_for_report(REPLY_DELAY)
    }
}

fn exchange(
    io: &mut impl IdentityIo,
    report_id: ReportId,
    delay: impl Fn(Duration),
) -> io::Result<Identity> {
    let request = request(report_id);
    let mut response = [0; SDK_REPORT_LEN];
    for _ in 0..MAX_ATTEMPTS {
        if io.send(&request)? != SDK_REPORT_LEN {
            return Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "short identity request",
            ));
        }
        delay(REPLY_DELAY);
        let count = io.receive(&mut response)?;
        if count == 0 {
            return Err(io::Error::new(io::ErrorKind::TimedOut, "no identity reply"));
        }
        if response[0] != report_id.byte() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "identity reply used a different HID report ID",
            ));
        }
        let reply = first_reply(&response[..count])
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid identity reply"))?;
        if let FirstReply::Fields { flags, tokens } = reply {
            let fields = tokens.map(<[u8]>::to_vec);
            let mut count = io.receive(&mut response)?;
            if count == 0 && io.wait_for_optional_reply()? {
                count = io.receive(&mut response)?;
            }
            return Ok(Identity {
                fields,
                feature_flag: flags != 0,
                build_stamp: match response.get(..count) {
                    Some(report) if report.first() == Some(&report_id.byte()) => {
                        second_reply(report)
                    }
                    _ => None,
                },
            });
        }
    }
    Err(io::Error::new(
        io::ErrorKind::InvalidData,
        "identity reply retry limit reached",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::VecDeque;

    struct Device {
        replies: VecDeque<&'static [u8]>,
        writes: usize,
    }
    impl IdentityIo for Device {
        fn send(&mut self, packet: &[u8; SDK_REPORT_LEN]) -> io::Result<usize> {
            assert_eq!(&packet[..2], &[1, COMMAND]);
            self.writes += 1;
            Ok(packet.len())
        }
        fn receive(&mut self, output: &mut [u8; SDK_REPORT_LEN]) -> io::Result<usize> {
            let packet = self.replies.pop_front().unwrap_or_default();
            output[..packet.len()].copy_from_slice(packet);
            Ok(packet.len())
        }

        fn wait_for_optional_reply(&self) -> io::Result<bool> {
            Ok(!self.replies.is_empty())
        }
    }

    #[test]
    fn identity_command_has_the_vendor_wire_header() {
        let packet = request(ReportId::Collection06);
        assert_eq!(&packet[..3], &[6, 0x0d, 0]);
        assert!(packet[3..].iter().all(|&byte| byte == 0));
    }

    #[test]
    fn parser_handles_padding_delimiters_and_retry() {
        assert_eq!(
            first_reply(b"\x01\x0d\x00\0\0\0controller:alt\0"),
            Ok(FirstReply::Retry)
        );
        assert_eq!(
            first_reply(b"\x01\x0d\x01\0\0\0M484,,01,KB,FR,model,version\0ignored"),
            Ok(FirstReply::Fields {
                flags: 1,
                tokens: [b"M484", b"01", b"KB", b"FR", b"model", b"version"]
            })
        );
        assert_eq!(first_reply(&[1, COMMAND]), Err(ReplyError::Truncated));
        assert_eq!(
            first_reply(b"\x01\x0d\0\0\0\0,,,\0"),
            Err(ReplyError::Truncated)
        );
    }

    #[test]
    fn reads_the_vendor_build_stamp_and_keeps_the_day_token() {
        assert_eq!(
            second_reply(b"\x01\x0d\0\0\0\0Nov  9 2023,14:03:02\0"),
            Some("2023/11/9 14:03:02".into()),
        );
        assert_eq!(
            second_reply(b"\x01\x0d\0\0\0\0Invalid 9 2023,14:03:02"),
            None
        );
    }

    #[test]
    fn exchanges_identity_including_a_retry_and_second_packet() {
        let mut device = Device {
            replies: VecDeque::from([
                &b"\x01\x0d\x00\0\0\0controller:alt"[..],
                &b"\x01\x0d\x01\0\0\0M484,01,KB,FR,model,version"[..],
                &b"\x01\x0d\x00\0\0\0Jan 1 2024,10:20:30"[..],
            ]),
            writes: 0,
        };
        let result = exchange(&mut device, ReportId::Default, |duration| {
            assert_eq!(duration, REPLY_DELAY)
        })
        .unwrap();
        assert_eq!(device.writes, 2);
        assert_eq!(result.fields[0], b"M484");
        assert!(result.feature_flag);
        assert_eq!(result.build_stamp.as_deref(), Some("2024/1/1 10:20:30"));
        assert!(result.model().is_none());
    }

    #[test]
    fn maps_the_k617m_observed_on_usb_to_the_vendor_catalog() {
        // The user provided decoded probe output. These bytes reconstruct the replies.
        let mut device = Device {
            replies: VecDeque::from([
                &b"\x01\x0d\0\0\0\0M484,20,KB,FR,7153_AUSHERGB,V1_11_27\0"[..],
                &b""[..],
                &b"\x01\x0d\0\0\0\0Apr  9 2024,11:08:13\0"[..],
            ]),
            writes: 0,
        };
        let result = exchange(&mut device, ReportId::Default, |_| {}).unwrap();
        assert_eq!(device.writes, 1);
        assert_eq!(result.controller(), b"M484");
        assert_eq!(result.layout_code(), b"7153_AUSHERGB");
        assert_eq!(result.firmware_version(), b"V1_11_27");
        assert!(!result.feature_flag);
        assert_eq!(result.build_stamp.as_deref(), Some("2024/4/9 11:08:13"));
        assert_eq!(result.model().unwrap().key_count, 61);
    }

    #[test]
    fn rejects_a_reply_from_the_wrong_report_collection() {
        let mut device = Device {
            replies: VecDeque::from([&b"\x06\x0d\0\0\0\0M484,20,KB,FR,7153_AUSHERGB,V1_11_27"[..]]),
            writes: 0,
        };
        assert_eq!(
            exchange(&mut device, ReportId::Default, |_| {})
                .unwrap_err()
                .kind(),
            io::ErrorKind::InvalidData
        );
    }

    #[test]
    fn parses_a_build_timestamp_without_assuming_the_second_command_byte() {
        assert_eq!(
            second_reply(b"\x01\x21\0\0\0\0Apr 9 2024,11:08:13"),
            Some("2024/4/9 11:08:13".into())
        );
    }

    #[test]
    fn does_not_use_a_build_timestamp_from_another_collection() {
        let mut device = Device {
            replies: VecDeque::from([
                &b"\x01\x0d\0\0\0\0M484,20,KB,FR,7153_AUSHERGB,V1_11_27"[..],
                &b"\x06\x0d\0\0\0\0Apr 9 2024,11:08:13"[..],
            ]),
            writes: 0,
        };
        let identity = exchange(&mut device, ReportId::Default, |_| {}).unwrap();
        assert_eq!(identity.build_stamp, None);
        assert_eq!(identity.firmware_version(), b"V1_11_27");
    }
}
