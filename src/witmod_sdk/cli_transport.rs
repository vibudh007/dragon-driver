use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::fd::OwnedFd;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::hid::{SDK_REPORT_LEN, frame_write_report};
use super::session::SdkSession;
use crate::keyboard::lighting::synchronize;

const READ: u8 = 1;
const WRITE: u8 = 2;
const WAIT: u8 = 3;
const NONBLOCKING: u8 = 4;
const MAX_WAIT_MS: u32 = 30_000;
const IPC_TIMEOUT: Duration = Duration::from_secs(32);

pub(super) struct CliTransport {
    child: Child,
    stream: UnixStream,
}

impl CliTransport {
    pub fn open(node: &Path) -> io::Result<Self> {
        let executable = std::env::current_exe()?.with_file_name("dragonctl");
        let (stream, child_endpoint) = UnixStream::pair()?;
        stream.set_read_timeout(Some(IPC_TIMEOUT))?;
        stream.set_write_timeout(Some(IPC_TIMEOUT))?;
        let child = Command::new(executable)
            .arg("--config-transport")
            .arg(node)
            .stdin(Stdio::from(OwnedFd::from(child_endpoint.try_clone()?)))
            .stdout(Stdio::from(OwnedFd::from(child_endpoint)))
            .spawn()?;
        let result = Self { child, stream };
        result.read_status()?;
        Ok(result)
    }

    pub fn read(&self, output: &mut [u8; SDK_REPORT_LEN]) -> io::Result<usize> {
        let mut stream = &self.stream;
        stream.write_all(&[READ])?;
        self.read_status()?;
        let count = usize::try_from(read_u32(&mut stream)?).unwrap_or(usize::MAX);
        let data = output.get_mut(..count).ok_or_else(invalid)?;
        stream.read_exact(data)?;
        Ok(count)
    }

    pub fn write(&self, report: &[u8; SDK_REPORT_LEN]) -> io::Result<usize> {
        let mut stream = &self.stream;
        stream.write_all(&[WRITE])?;
        stream.write_all(report)?;
        self.read_status()?;
        Ok(read_u32(&mut stream)? as usize)
    }

    pub fn wait(&self, timeout: Duration) -> io::Result<bool> {
        let mut stream = &self.stream;
        let millis = timeout
            .as_nanos()
            .div_ceil(1_000_000)
            .min(u128::from(MAX_WAIT_MS)) as u32;
        stream.write_all(&[WAIT])?;
        stream.write_all(&millis.to_le_bytes())?;
        self.read_status()?;
        Ok(read_byte(&mut stream)? != 0)
    }

    pub fn set_nonblocking(&self, enabled: bool) -> io::Result<()> {
        (&self.stream).write_all(&[NONBLOCKING, u8::from(enabled)])?;
        self.read_status()
    }

    fn read_status(&self) -> io::Result<()> {
        match read_u32(&mut &self.stream)? {
            0 => Ok(()),
            errno => Err(io::Error::from_raw_os_error(errno as i32)),
        }
    }
}

impl Drop for CliTransport {
    fn drop(&mut self) {
        let _ = self.stream.shutdown(Shutdown::Both);
        let deadline = Instant::now() + Duration::from_millis(200);
        loop {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                Ok(None) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                _ => break,
            }
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

pub fn serve(node: &Path, input: impl Read, mut output: impl Write) -> io::Result<()> {
    let result = (|| {
        let sdk = SdkSession::initialize_shared_broker()?;
        let interface = sdk
            .devices()
            .iter()
            .find(|device| device.node() == node)
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?;
        for _ in 0..10 {
            match sdk.open(interface) {
                Err(error) if error.kind() == io::ErrorKind::ResourceBusy => {
                    std::thread::sleep(Duration::from_millis(100));
                }
                result => return result,
            }
        }
        sdk.open(interface)
    })();
    write_status(&mut output, &result)?;
    output.flush()?;
    serve_device(result?, input, output)
}

fn serve_device(
    mut device: super::hidraw::HidrawDevice,
    input: impl Read,
    output: impl Write,
) -> io::Result<()> {
    let id = device.layout().output_id;
    let [enter, exit] = [true, false]
        .map(|enabled| frame_write_report(synchronize(enabled), id).expect("valid sync report"));
    let mut synchronized = false;
    let result = relay_reports(&mut device, &mut synchronized, enter, exit, input, output);
    if synchronized {
        let cleanup = device.write_framed_report(&exit);
        result.and(cleanup.map(|_| ()))
    } else {
        result
    }
}

fn relay_reports(
    device: &mut super::hidraw::HidrawDevice,
    synchronized: &mut bool,
    enter: [u8; SDK_REPORT_LEN],
    exit: [u8; SDK_REPORT_LEN],
    mut input: impl Read,
    mut output: impl Write,
) -> io::Result<()> {
    loop {
        let op = match read_byte(&mut input) {
            Ok(op) => op,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        };
        match op {
            READ => {
                let mut report = [0; SDK_REPORT_LEN];
                let result = device.read_report(&mut report);
                write_status(&mut output, &result)?;
                if let Ok(size) = result {
                    output.write_all(&(size as u32).to_le_bytes())?;
                    output.write_all(&report[..size])?;
                }
            }
            WRITE => {
                let mut report = [0; SDK_REPORT_LEN];
                input.read_exact(&mut report)?;
                let result = device.write_framed_report(&report);
                if report == enter {
                    *synchronized = true;
                } else if report == exit
                    && result.as_ref().is_ok_and(|size| *size >= SDK_REPORT_LEN)
                {
                    *synchronized = false;
                }
                write_status(&mut output, &result)?;
                if let Ok(size) = result {
                    output.write_all(&(size as u32).to_le_bytes())?;
                }
            }
            WAIT => {
                let millis = read_u32(&mut input)?;
                let result = if millis <= MAX_WAIT_MS {
                    device.wait_for_report(Duration::from_millis(u64::from(millis)))
                } else {
                    Err(invalid())
                };
                write_status(&mut output, &result)?;
                if let Ok(ready) = result {
                    output.write_all(&[u8::from(ready)])?;
                }
            }
            NONBLOCKING => {
                let result = device.set_nonblocking(read_byte(&mut input)? != 0);
                write_status(&mut output, &result)?;
            }
            _ => return Err(invalid()),
        }
        output.flush()?;
    }
}

fn read_byte(input: &mut impl Read) -> io::Result<u8> {
    let mut byte = [0];
    input.read_exact(&mut byte)?;
    Ok(byte[0])
}

fn read_u32(input: &mut impl Read) -> io::Result<u32> {
    let mut bytes = [0; 4];
    input.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn write_status<T>(output: &mut impl Write, result: &io::Result<T>) -> io::Result<()> {
    let errno = result.as_ref().err().map_or(0, |error| {
        error.raw_os_error().unwrap_or(match error.kind() {
            io::ErrorKind::PermissionDenied => libc::EACCES,
            io::ErrorKind::NotFound | io::ErrorKind::NotConnected => libc::ENODEV,
            io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData => libc::EINVAL,
            io::ErrorKind::TimedOut => libc::ETIMEDOUT,
            io::ErrorKind::ResourceBusy => libc::EBUSY,
            _ => libc::EIO,
        })
    });
    output.write_all(&errno.to_le_bytes())
}

fn invalid() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "Invalid CLI transport message")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closing_the_transport_allows_the_relay_to_finish_after_eof() {
        let (stream, peer) = UnixStream::pair().unwrap();
        let mut child = Command::new("/bin/sh")
            .args(["-c", "read -r line || { sleep 0.02; printf cleaned; }"])
            .stdin(Stdio::from(OwnedFd::from(peer)))
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let mut output = child.stdout.take().unwrap();
        drop(CliTransport { stream, child });
        let mut marker = String::new();
        output.read_to_string(&mut marker).unwrap();
        assert_eq!(marker, "cleaned");
    }

    #[test]
    fn relay_preserves_report_bytes_and_length() {
        let (device, mut fixture) = super::super::hidraw::HidrawDevice::fixture();
        fixture.write_all(&[1, 7, 9]).unwrap();
        let mut request = vec![NONBLOCKING, 1, WRITE];
        request.extend([7; SDK_REPORT_LEN]);
        request.extend([WAIT, 0, 0, 0, 0, READ]);
        let mut output = Vec::new();
        serve_device(device, request.as_slice(), &mut output).unwrap();
        let mut report = [0; SDK_REPORT_LEN];
        fixture.read_exact(&mut report).unwrap();
        assert_eq!(report, [7; SDK_REPORT_LEN]);
        let mut reply = output.as_slice();
        assert_eq!(read_u32(&mut reply).unwrap(), 0); // Nonblocking
        assert_eq!(read_u32(&mut reply).unwrap(), 0); // Write
        assert_eq!(read_u32(&mut reply).unwrap(), SDK_REPORT_LEN as u32);
        assert_eq!(read_u32(&mut reply).unwrap(), 0); // Wait
        assert_eq!(read_byte(&mut reply).unwrap(), 1);
        assert_eq!(read_u32(&mut reply).unwrap(), 0); // Read
        assert_eq!(read_u32(&mut reply).unwrap(), 3);
        assert_eq!(reply, &[1, 7, 9]);
    }

    #[test]
    fn disconnect_releases_synchronized_lighting() {
        let (device, mut fixture) = super::super::hidraw::HidrawDevice::fixture();
        let id = device.layout().output_id;
        let enter = frame_write_report(synchronize(true), id).unwrap();
        let exit = frame_write_report(synchronize(false), id).unwrap();
        let mut request = vec![WRITE];
        request.extend(enter);
        serve_device(device, request.as_slice(), io::sink()).unwrap();
        let mut reports = [0; 2 * SDK_REPORT_LEN];
        fixture.read_exact(&mut reports).unwrap();
        assert_eq!(&reports[..SDK_REPORT_LEN], &enter);
        assert_eq!(&reports[SDK_REPORT_LEN..], &exit);
    }
}
