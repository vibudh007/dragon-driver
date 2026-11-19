use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::num::NonZeroU16;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd};
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::time::{Duration, Instant};

use super::discovery::{HIDRAW_CLASS, device_number};
use super::hid::{FrameError, ReportId, SDK_REPORT_LEN, frame_write_report};

#[derive(Clone, Copy, Debug)]
pub struct ReportLayout {
    pub input_len: NonZeroU16,
    pub output_len: NonZeroU16,
    pub output_id: ReportId,
}

#[derive(Debug)]
pub enum WriteError {
    InvalidFrame(FrameError),
    Io(io::Error),
}

pub struct HidrawDevice {
    link: Link,
    #[cfg(feature = "daemon")]
    lease: Option<std::os::unix::net::UnixStream>,
    input: Vec<u8>,
    padded_output: Vec<u8>,
    layout: ReportLayout,
}

enum Link {
    File(File),
    Closed,
    #[cfg(feature = "daemon")]
    Cli(super::cli_transport::CliTransport),
}

impl HidrawDevice {
    #[cfg(feature = "daemon")]
    pub(super) fn retain_lease(&mut self, lease: std::os::unix::net::UnixStream) {
        self.lease = Some(lease);
    }

    #[cfg(all(test, feature = "daemon"))]
    pub(super) fn fixture() -> (Self, std::os::unix::net::UnixStream) {
        let (device, fixture) = std::os::unix::net::UnixStream::pair().unwrap();
        let file = File::from(std::os::fd::OwnedFd::from(device));
        let length = NonZeroU16::new(SDK_REPORT_LEN as u16).unwrap();
        let layout = ReportLayout {
            input_len: length,
            output_len: length,
            output_id: ReportId::Default,
        };
        (
            Self {
                #[cfg(feature = "daemon")]
                lease: None,
                link: Link::File(file),
                input: vec![0; SDK_REPORT_LEN],
                padded_output: Vec::new(),
                layout,
            },
            fixture,
        )
    }

    pub fn open(path: &Path, layout: ReportLayout) -> io::Result<Self> {
        let name = node_name(path)?;
        let number = fs::read_to_string(Path::new(HIDRAW_CLASS).join(name).join("dev"))?;
        let number = device_number(&number).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid hidraw device number")
        })?;
        Self::open_checked(path, layout, number, false)
    }

    pub(super) fn open_checked(
        path: &Path,
        layout: ReportLayout,
        expected_number: libc::dev_t,
        nonblocking: bool,
    ) -> io::Result<Self> {
        node_name(path)?;
        verify_node(&fs::symlink_metadata(path)?, expected_number)?;
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NOCTTY | libc::O_NONBLOCK)
            .open(path)?;
        Self::from_file(file, layout, expected_number, nonblocking)
    }

    pub(super) fn from_file(
        file: File,
        layout: ReportLayout,
        expected_number: libc::dev_t,
        nonblocking: bool,
    ) -> io::Result<Self> {
        verify_node(&file.metadata()?, expected_number)?;
        let padded_len = usize::from(layout.output_len.get());
        let mut device = Self {
            #[cfg(feature = "daemon")]
            lease: None,
            link: Link::File(file),
            input: vec![0; usize::from(layout.input_len.get())],
            padded_output: if padded_len > SDK_REPORT_LEN {
                vec![0; padded_len]
            } else {
                Vec::new()
            },
            layout,
        };
        device.set_nonblocking(nonblocking)?;
        Ok(device)
    }

    #[cfg(feature = "daemon")]
    pub fn into_file(self) -> io::Result<File> {
        match self.link {
            Link::File(file) => Ok(file),
            _ => Err(io::Error::other("The session has no local HID handle")),
        }
    }

    pub(crate) fn close(&mut self) {
        self.link = Link::Closed;
        #[cfg(feature = "daemon")]
        {
            self.lease = None;
        }
    }

    #[cfg(feature = "daemon")]
    pub(super) fn open_cli(
        path: &Path,
        layout: ReportLayout,
        nonblocking: bool,
    ) -> io::Result<Self> {
        let link = super::cli_transport::CliTransport::open(path)?;
        link.set_nonblocking(nonblocking)?;
        Ok(Self {
            #[cfg(feature = "daemon")]
            lease: None,
            link: Link::Cli(link),
            input: Vec::new(),
            padded_output: Vec::new(),
            layout,
        })
    }

    pub fn set_nonblocking(&mut self, enabled: bool) -> io::Result<()> {
        let file = match &self.link {
            Link::File(file) => file,
            Link::Closed => return Err(disconnected()),
            #[cfg(feature = "daemon")]
            Link::Cli(link) => return link.set_nonblocking(enabled),
        };
        let fd = file.as_raw_fd();
        // The File owns this descriptor throughout both fcntl calls.
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags == -1 {
            return Err(io::Error::last_os_error());
        }
        let new_flags = if enabled {
            flags | libc::O_NONBLOCK
        } else {
            flags & !libc::O_NONBLOCK
        };
        if flags != new_flags && unsafe { libc::fcntl(fd, libc::F_SETFL, new_flags) } == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    pub fn read_report(&mut self, output: &mut [u8; SDK_REPORT_LEN]) -> io::Result<usize> {
        match &mut self.link {
            Link::File(file) => read_report_from(file, &mut self.input, output),
            Link::Closed => Err(disconnected()),
            #[cfg(feature = "daemon")]
            Link::Cli(link) => link.read(output),
        }
    }

    pub(crate) fn wait_for_report(&self, timeout: Duration) -> io::Result<bool> {
        match &self.link {
            Link::File(file) => wait_readable(file.as_fd(), timeout),
            Link::Closed => Err(disconnected()),
            #[cfg(feature = "daemon")]
            Link::Cli(link) => link.wait(timeout),
        }
    }

    pub(crate) fn layout(&self) -> ReportLayout {
        self.layout
    }

    pub fn write_report(&mut self, report: [u8; SDK_REPORT_LEN]) -> Result<usize, WriteError> {
        let framed =
            frame_write_report(report, self.layout.output_id).map_err(WriteError::InvalidFrame)?;
        self.write_framed_report(&framed).map_err(WriteError::Io)
    }

    pub(super) fn write_framed_report(
        &mut self,
        report: &[u8; SDK_REPORT_LEN],
    ) -> io::Result<usize> {
        match &mut self.link {
            Link::File(file) => write_report_to(file, report, &mut self.padded_output),
            Link::Closed => Err(disconnected()),
            #[cfg(feature = "daemon")]
            Link::Cli(link) => link.write(report),
        }
    }
}

fn disconnected() -> io::Error {
    io::Error::from(io::ErrorKind::NotConnected)
}

fn node_name(path: &Path) -> io::Result<&str> {
    let invalid = || io::Error::new(io::ErrorKind::InvalidInput, "expected a /dev/hidrawN path");
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(invalid)?;
    let valid = name.strip_prefix("hidraw").is_some_and(|number| {
        !number.is_empty() && number.bytes().all(|byte| byte.is_ascii_digit())
    });
    if path.parent() != Some(Path::new("/dev")) || !valid {
        return Err(invalid());
    }
    Ok(name)
}

fn verify_node(metadata: &Metadata, expected: libc::dev_t) -> io::Result<()> {
    if !metadata.file_type().is_char_device() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected a HID character device",
        ));
    }
    if metadata.rdev() != expected {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            "the HID device node changed",
        ));
    }
    Ok(())
}

fn wait_readable(fd: BorrowedFd<'_>, timeout: Duration) -> io::Result<bool> {
    let started = Instant::now();
    let mut descriptor = libc::pollfd {
        fd: fd.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    loop {
        let remaining = timeout.saturating_sub(started.elapsed());
        let milliseconds = remaining
            .as_nanos()
            .div_ceil(1_000_000)
            .min(i32::MAX as u128) as i32;
        // The borrowed descriptor and single pollfd remain valid throughout the syscall.
        let result = unsafe { libc::poll(&mut descriptor, 1, milliseconds) };
        if result == 0 {
            return Ok(false);
        }
        if result > 0 {
            if descriptor.revents & libc::POLLIN != 0 {
                return Ok(true);
            }
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "HID device became unavailable",
            ));
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
        if started.elapsed() >= timeout {
            return Ok(false);
        }
    }
}

fn read_report_from(
    reader: &mut impl Read,
    input: &mut [u8],
    output: &mut [u8; SDK_REPORT_LEN],
) -> io::Result<usize> {
    loop {
        match reader.read(input) {
            Ok(count) => {
                let copied = count.min(output.len());
                output[..copied].copy_from_slice(&input[..copied]);
                return Ok(copied);
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return Ok(0),
            Err(error) => return Err(error),
        }
    }
}

fn write_report_to(
    writer: &mut impl Write,
    report: &[u8; SDK_REPORT_LEN],
    padded: &mut [u8],
) -> io::Result<usize> {
    fn write_once(writer: &mut impl Write, bytes: &[u8]) -> io::Result<usize> {
        loop {
            match writer.write(bytes) {
                Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
                result => return result,
            }
        }
    }

    if padded.len() <= report.len() {
        return write_once(writer, report);
    }
    padded[..report.len()].copy_from_slice(report);
    write_once(writer, padded)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_keeps_the_first_data_byte_and_leaves_unused_output_unchanged() {
        let mut input = [0; 128];
        let mut output = [0xff; SDK_REPORT_LEN];
        assert_eq!(
            read_report_from(&mut &[0, 0x2a][..], &mut input, &mut output).unwrap(),
            2
        );
        assert_eq!(&output[..3], &[0, 0x2a, 0xff]);
        assert_eq!(
            read_report_from(&mut &[6; 65][..], &mut input, &mut output).unwrap(),
            64
        );
    }

    #[test]
    fn write_pads_long_reports_and_never_splits_a_short_write() {
        let report = [1; SDK_REPORT_LEN];
        let mut bytes = Vec::new();
        let mut padded = [0; 66];
        assert_eq!(
            write_report_to(&mut bytes, &report, &mut padded).unwrap(),
            66
        );
        assert_eq!(&bytes[..64], &report);
        assert_eq!(&bytes[64..], &[0, 0]);
        let mut small = [0; 16];
        assert_eq!(
            write_report_to(&mut small.as_mut_slice(), &report, &mut []).unwrap(),
            16
        );
        assert_eq!(small, [1; 16]);
    }
}
#[test]
fn polls_only_until_input_is_ready_or_the_timeout_expires() {
    use std::os::unix::net::UnixStream;
    let (reader, mut writer) = UnixStream::pair().unwrap();
    assert!(!wait_readable(reader.as_fd(), Duration::ZERO).unwrap());
    writer.write_all(&[1]).unwrap();
    assert!(wait_readable(reader.as_fd(), Duration::ZERO).unwrap());
}
