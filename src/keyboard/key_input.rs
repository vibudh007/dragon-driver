use std::fs::{self, File};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::path::Path;

use crate::witmod_sdk::discovery::ConfigInterface;

pub(super) struct Input {
    streams: Vec<KeyStream>,
}

struct KeyStream {
    file: File,
    scan: Option<u8>,
}

#[repr(C)]
struct InputEvent {
    time: libc::timeval,
    kind: u16,
    code: u16,
    value: i32,
}

impl Input {
    pub fn open(interface: &ConfigInterface) -> io::Result<Self> {
        let port = interface
            .usb_device_path()
            .ok_or_else(|| io::Error::other("Missing USB device path"))?;
        let mut streams = Vec::new();
        let mut denied = false;
        for entry in fs::read_dir("/sys/class/input")? {
            let entry = entry?;
            let name = entry.file_name();
            let Some(number) = name.to_str().and_then(|name| name.strip_prefix("event")) else {
                continue;
            };
            if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
                continue;
            }
            let path = entry.path();
            if !fs::canonicalize(&path).is_ok_and(|resolved| resolved.starts_with(port)) {
                continue;
            }
            // Relative pointer events cannot be the selected keyboard's key source.
            // A bitmap with only low key bits clear describes e.g. mouse buttons.
            let bits = fs::read_to_string(path.join("device/capabilities/key")).unwrap_or_default();
            if bits
                .split_whitespace()
                .last()
                .and_then(|value| u64::from_str_radix(value, 16).ok())
                .is_none_or(|mask| mask & (1_u64 << 30) == 0)
            {
                continue;
            } // KEY_A
            let number = fs::read_to_string(path.join("dev"))?;
            let (major, minor) = number
                .trim()
                .split_once(':')
                .ok_or_else(|| io::Error::other("Malformed input device number"))?;
            let (major, minor): (u32, u32) = (
                major.parse().map_err(io::Error::other)?,
                minor.parse().map_err(io::Error::other)?,
            );
            match File::options()
                .read(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_NOCTTY)
                .open(Path::new("/dev/input").join(name))
            {
                Ok(file) => {
                    let metadata = file.metadata()?;
                    if metadata.file_type().is_char_device()
                        && metadata.rdev() == libc::makedev(major, minor)
                        && fs::canonicalize(&path).is_ok_and(|resolved| resolved.starts_with(port))
                    {
                        streams.push(KeyStream { file, scan: None });
                    }
                }
                Err(error) if error.kind() == io::ErrorKind::PermissionDenied => denied = true,
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                Err(error) => return Err(error),
            }
        }
        if streams.is_empty() {
            return Err(if denied {
                io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "Reactive lighting needs read access to this keyboard’s input interface",
                )
            } else {
                io::Error::new(
                    io::ErrorKind::NotFound,
                    "This keyboard has no readable key-event interface",
                )
            });
        }
        Ok(Self { streams })
    }

    pub fn newest_press(&mut self) -> io::Result<Option<u8>> {
        let mut pressed = None;
        for stream in &mut self.streams {
            for _ in 0..128 {
                let mut event = InputEvent {
                    time: libc::timeval {
                        tv_sec: 0,
                        tv_usec: 0,
                    },
                    kind: 0,
                    code: 0,
                    value: 0,
                };
                let size = std::mem::size_of::<InputEvent>();
                // The open evdev descriptor returns an array of native input_event records.
                let n = unsafe {
                    libc::read(
                        stream.file.as_raw_fd(),
                        (&mut event as *mut InputEvent).cast(),
                        size,
                    )
                };
                if n < 0 {
                    let error = io::Error::last_os_error();
                    match error.kind() {
                        io::ErrorKind::WouldBlock => break,
                        io::ErrorKind::Interrupted => continue,
                        _ => return Err(error),
                    }
                }
                if n as usize != size {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Keyboard input disconnected",
                    ));
                }
                pressed = sample(&mut stream.scan, event.kind, event.code, event.value).or(pressed);
            }
        }
        Ok(pressed)
    }
}

fn sample(scan: &mut Option<u8>, kind: u16, code: u16, value: i32) -> Option<u8> {
    match (kind, code, value) {
        (4, 4, value) => {
            // MSC_SCAN on USB HID combines the usage page and usage.
            *scan = if value & !0xff == 0x0007_0000 {
                Some(value as u8)
            } else {
                None
            };
            None
        }
        (1, _, down) => scan.take().filter(|_| down == 1),
        (0, _, _) => {
            *scan = None;
            None
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_hid_scans_are_paired_with_press_and_skip_repeat() {
        let mut scan = None;
        assert_eq!(sample(&mut scan, 4, 4, 0x7001a), None);
        assert_eq!(sample(&mut scan, 1, 17, 1), Some(26));
        assert_eq!(sample(&mut scan, 4, 4, 0x7001a), None);
        assert_eq!(sample(&mut scan, 1, 17, 2), None);
        assert_eq!(sample(&mut scan, 4, 4, 0xc001a), None);
        assert_eq!(sample(&mut scan, 1, 17, 1), None);
        assert_eq!(sample(&mut scan, 4, 4, 0x7001a), None);
        assert_eq!(sample(&mut scan, 0, 3, 0), None);
        assert_eq!(sample(&mut scan, 1, 17, 1), None);
    }
}
