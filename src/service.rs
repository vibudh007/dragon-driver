//! Local client and protocol for the unprivileged Dragon Driver desktop service.
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::{fs::OpenOptionsExt, net::UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use gio::prelude::*;
use serde::{Deserialize, Serialize};

use crate::keyboard::lighting::{Effect, Region, RgbFrame};

const VERSION: u8 = 2;
pub const OPEN: u8 = 1;
pub const AUDIO: u8 = 2;
pub const PALETTE: u8 = 3;
pub const PING: u8 = 4;
pub const PALETTE_READ: u8 = 5;
pub const RELEASE_SYSTEM_ACCESS: u8 = 6;
pub const WALLPAPER: u8 = 7;
pub const WALLPAPER_SUPPORT_V1: u8 = 8;
pub const WALLPAPER_SUPPORT: u8 = 11;
pub const AUDIO_BARS: u8 = 12;
pub const AUDIO_SUPPORT: u8 = 13;
pub const OPEN_SHARED: u8 = 14;
pub const REACTIVE: u8 = 15;
pub const LIGHTING_PERSISTENCE_SUPPORT: u8 = 16;
pub const MAX_WALLPAPER_BYTES: usize = 32 * 1024 * 1024;
const MAX_UPDATE: u32 = 4096;
pub const OK: u8 = 0;
const DENIED: u8 = 1;
const MISSING: u8 = 2;
const BUSY: u8 = 3;
const INVALID: u8 = 4;
const FAILED: u8 = 5;
const TIMEOUT: Duration = Duration::from_secs(2);

pub fn uid() -> libc::uid_t {
    // SAFETY: geteuid takes no arguments and returns the process's effective user ID.
    unsafe { libc::geteuid() }
}

pub fn socket_path() -> io::Result<PathBuf> {
    let runtime = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
    let runtime = runtime.filter(|path| path.is_absolute()).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "the desktop runtime directory is unavailable",
        )
    })?;
    Ok(runtime.join("dragon-driver/daemon.sock"))
}

pub fn authenticate(stream: &UnixStream) -> io::Result<()> {
    authenticate_uid(stream, uid())
}

pub fn authenticate_uid(stream: &UnixStream, expected_uid: u32) -> io::Result<()> {
    let mut credentials = libc::ucred {
        pid: 0,
        uid: 0,
        gid: 0,
    };
    let mut size = std::mem::size_of_val(&credentials) as libc::socklen_t;
    // SAFETY: the live socket and writable credential buffer match Linux SO_PEERCRED.
    let result = unsafe {
        libc::getsockopt(
            stream.as_raw_fd(),
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            (&raw mut credentials).cast(),
            &raw mut size,
        )
    };
    if result != 0 {
        return Err(io::Error::last_os_error());
    }
    if credentials.uid != expected_uid {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unexpected session user",
        ));
    }
    Ok(())
}

pub fn fd_connection(stream: &UnixStream) -> io::Result<gio::UnixConnection> {
    // GIO switches the shared description to nonblocking, so callers finish their
    // standard-library reads and writes before using this ancillary connection.
    let socket = gio::Socket::from_fd(stream.try_clone()?.into()).map_err(io::Error::other)?;
    socket.set_timeout(TIMEOUT.as_secs() as u32);
    socket
        .connection_factory_create_connection()
        .downcast()
        .map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "expected a local Unix connection",
            )
        })
}

pub fn read_request(stream: &mut UnixStream) -> io::Result<(u8, u32)> {
    let mut packet = [0; 6];
    stream.read_exact(&mut packet)?;
    let [version, command, a, b, c, d] = packet;
    let index = u32::from_le_bytes([a, b, c, d]);
    if version != VERSION
        || !match command {
            OPEN | OPEN_SHARED => true,
            AUDIO
            | AUDIO_BARS
            | AUDIO_SUPPORT
            | PING
            | PALETTE_READ
            | LIGHTING_PERSISTENCE_SUPPORT
            | RELEASE_SYSTEM_ACCESS
            | WALLPAPER
            | WALLPAPER_SUPPORT_V1..=WALLPAPER_SUPPORT => index == 0,
            PALETTE | REACTIVE => (1..=MAX_UPDATE).contains(&index),
            _ => false,
        }
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid service request",
        ));
    }
    Ok((command, index))
}

pub fn response(stream: &mut UnixStream, result: io::Result<()>) -> io::Result<()> {
    let status = match result.map_err(|error| error.kind()) {
        Ok(()) => OK,
        Err(io::ErrorKind::PermissionDenied) => DENIED,
        Err(io::ErrorKind::NotFound | io::ErrorKind::NotConnected) => MISSING,
        Err(io::ErrorKind::ResourceBusy) => BUSY,
        Err(io::ErrorKind::InvalidInput | io::ErrorKind::InvalidData) => INVALID,
        Err(_) => FAILED,
    };
    stream.write_all(&[status])
}

fn request(stream: &mut UnixStream, command: u8, index: u32) -> io::Result<()> {
    send_request(stream, command, index, &[])?;
    read_response(stream)
}

fn send_request(stream: &mut UnixStream, command: u8, index: u32, body: &[u8]) -> io::Result<()> {
    let [a, b, c, d] = index.to_le_bytes();
    stream.set_read_timeout(Some(TIMEOUT))?;
    stream.set_write_timeout(Some(TIMEOUT))?;
    stream.write_all(&[VERSION, command, a, b, c, d])?;
    stream.write_all(body)
}

fn read_response(stream: &mut UnixStream) -> io::Result<()> {
    let mut answer = [0];
    stream.read_exact(&mut answer)?;
    match answer[0] {
        OK => Ok(()),
        DENIED => Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "keyboard access needs approval",
        )),
        MISSING => Err(io::Error::new(
            io::ErrorKind::NotConnected,
            "the requested device is unavailable",
        )),
        BUSY => Err(io::Error::new(
            io::ErrorKind::ResourceBusy,
            "the requested resource is already in use",
        )),
        INVALID => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "the desktop service rejected the request",
        )),
        _ => Err(io::Error::other(
            "the desktop service could not complete the request",
        )),
    }
}

#[derive(Deserialize, Serialize)]
pub enum PaletteUpdate {
    Slot {
        slot: u8,
        frame: Option<Box<RgbFrame>>,
    },
    Effect(u8),
    Reset,
    Lighting {
        region: Region,
        effect: Effect,
    },
    ConfirmedSlot {
        slot: u8,
        frame: Box<RgbFrame>,
        effect: Effect,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ReactiveMode {
    Wallpaper,
    Audio {
        colors: crate::keyboard::lighting::AudioPalette,
        follow_wallpaper: bool,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReactiveTarget {
    pub node: u32,
    pub model: String,
    pub firmware: String,
    pub vendor: u16,
    pub product: u16,
}

impl ReactiveTarget {
    pub fn new(
        path: &Path,
        model: &str,
        firmware: &str,
        usb: crate::witmod_sdk::catalog::UsbId,
    ) -> Option<Self> {
        Some(Self {
            node: index(path)?,
            model: model.into(),
            firmware: firmware.into(),
            vendor: usb.vendor,
            product: usb.product,
        })
    }

    pub fn same_model(&self, other: &Self) -> bool {
        self.model == other.model
            && self.firmware == other.firmware
            && self.vendor == other.vendor
            && self.product == other.product
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ReactiveRequest {
    Status,
    Start(ReactiveMode),
    Stop,
    AudioColors(crate::keyboard::lighting::AudioPalette),
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ReactiveStatus {
    pub mode: Option<ReactiveMode>,
    pub stopping: bool,
    pub starting: bool,
    pub frame: Option<Box<RgbFrame>>,
    pub message: Option<String>,
}

pub fn reactive(target: &ReactiveTarget, command: ReactiveRequest) -> io::Result<ReactiveStatus> {
    let body = encode_reactive(&(target, command))?;
    let mut stream = UnixStream::connect(socket_path()?)?;
    authenticate(&stream)?;
    send_request(&mut stream, REACTIVE, body.len() as u32, body.as_bytes())?;
    read_response(&mut stream)?;
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    read_reactive(&mut stream, u32::from_le_bytes(length))
}

fn encode_reactive(value: &impl Serialize) -> io::Result<String> {
    let body = ron::to_string(value).map_err(io::Error::other)?;
    if body.len() > MAX_UPDATE as usize {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    Ok(body)
}

pub fn read_reactive<T: serde::de::DeserializeOwned>(
    stream: &mut UnixStream,
    length: u32,
) -> io::Result<T> {
    if !(1..=MAX_UPDATE).contains(&length) {
        return Err(io::Error::from(io::ErrorKind::InvalidData));
    }
    let mut body = vec![0; length as usize];
    stream.read_exact(&mut body)?;
    ron::de::from_bytes(&body).map_err(io::Error::other)
}

pub fn write_reactive(stream: &mut UnixStream, status: ReactiveStatus) -> io::Result<()> {
    let body = encode_reactive(&status)?;
    response(stream, Ok(()))?;
    stream.write_all(&(body.len() as u32).to_le_bytes())?;
    stream.write_all(body.as_bytes())
}

pub fn require_user_service() -> io::Result<()> {
    let result = (|| {
        let mut stream = UnixStream::connect(socket_path()?)?;
        authenticate(&stream)?;
        request(&mut stream, PING, 0)
    })();
    result.map_err(|_| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Set up the Dragon Driver service once to use the keyboard",
        )
    })
}

pub fn require_lighting_persistence() -> io::Result<()> {
    let mut stream = UnixStream::connect(socket_path()?)?;
    authenticate(&stream)?;
    request(&mut stream, LIGHTING_PERSISTENCE_SUPPORT, 0).map_err(|_| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Update the Dragon Driver desktop companion to continue",
        )
    })
}

pub fn read_saved_palettes() -> io::Result<Vec<u8>> {
    let mut stream = UnixStream::connect(socket_path()?)?;
    authenticate(&stream)?;
    request(&mut stream, PALETTE_READ, 0)?;
    let mut length = [0; 4];
    stream.read_exact(&mut length)?;
    let length = u32::from_le_bytes(length);
    if u64::from(length) > crate::palettes::MAX_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Saved palette response is too large",
        ));
    }
    let mut body = vec![0; length as usize];
    stream.read_exact(&mut body)?;
    Ok(body)
}

pub fn update_palette(model: &str, firmware: &str, update: PaletteUpdate) -> io::Result<()> {
    let body = ron::to_string(&(model, firmware, update)).map_err(io::Error::other)?;
    let length = u32::try_from(body.len())
        .ok()
        .filter(|length| *length <= MAX_UPDATE)
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "Palette update is too large")
        })?;
    let mut stream = UnixStream::connect(socket_path()?)?;
    authenticate(&stream)?;
    send_request(&mut stream, PALETTE, length, body.as_bytes())?;
    read_response(&mut stream)
}

pub fn read_palette_update(
    stream: &mut UnixStream,
    length: u32,
) -> io::Result<(String, String, PaletteUpdate)> {
    if !(1..=MAX_UPDATE).contains(&length) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid palette update size",
        ));
    }
    let mut body = vec![0; length as usize];
    stream.read_exact(&mut body)?;
    let (model, firmware, update): (String, String, _) = ron::de::from_bytes(&body)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    if crate::keyboard::models::find(&model).is_none() || firmware.len() > 128 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Invalid keyboard identity",
        ));
    }
    Ok((model, firmware, update))
}

fn index(path: &Path) -> Option<u32> {
    if path.parent()? != Path::new("/dev") {
        return None;
    }
    path.file_name()?
        .to_str()?
        .strip_prefix("hidraw")?
        .parse()
        .ok()
}

pub fn open_keyboard(path: &Path) -> io::Result<Option<File>> {
    open_remote_keyboard(path, socket_path(), uid(), OPEN).map(|file| file.map(|(file, _)| file))
}

pub fn open_shared_keyboard(path: &Path) -> io::Result<Option<(File, UnixStream)>> {
    open_remote_keyboard(path, socket_path(), uid(), OPEN_SHARED)
}

pub fn config_lock_path() -> io::Result<PathBuf> {
    Ok(socket_path()?.with_file_name("config.lock"))
}

pub fn config_guard() -> io::Result<File> {
    let lock = open_config_file(&config_lock_path()?, false)?;
    lock.lock()?;
    Ok(lock)
}

pub fn prepare_config_lock() -> io::Result<()> {
    open_config_file(&config_lock_path()?, true).map(drop)
}

fn open_config_file(path: &Path, create: bool) -> io::Result<File> {
    let lock = OpenOptions::new()
        .read(true)
        .write(create)
        .create(create)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !lock.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "invalid configuration lock file",
        ));
    }
    Ok(lock)
}

pub fn open_system_keyboard(path: &Path) -> io::Result<Option<File>> {
    open_remote_keyboard(
        path,
        Ok(PathBuf::from(format!("/run/dragon-driver/{}.sock", uid()))),
        0,
        OPEN,
    )
    .map(|file| file.map(|(file, _)| file))
}

pub fn release_system_access() -> io::Result<()> {
    let mut stream = UnixStream::connect(format!("/run/dragon-driver/{}.sock", uid()))?;
    authenticate_uid(&stream, 0)?;
    request(&mut stream, RELEASE_SYSTEM_ACCESS, 0)
}

fn open_remote_keyboard(
    path: &Path,
    socket: io::Result<PathBuf>,
    owner: u32,
    operation: u8,
) -> io::Result<Option<(File, UnixStream)>> {
    let index = index(path)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid keyboard path"))?;
    let mut stream = match socket.and_then(UnixStream::connect) {
        Ok(stream) => stream,
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) =>
        {
            return Ok(None);
        }
        Err(error) => return Err(error),
    };
    authenticate_uid(&stream, owner)?;
    request(&mut stream, operation, index)?;
    let connection = fd_connection(&stream)?;
    let fd = connection
        .receive_fd(gio::Cancellable::NONE)
        .map_err(io::Error::other)?;
    // SAFETY: receive_fd returns a new descriptor owned by the caller.
    let file = File::from(unsafe { OwnedFd::from_raw_fd(fd) });
    // SAFETY: file owns the descriptor throughout fcntl. Preserve close-on-exec on received handles.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(Some((file, stream)))
}

pub struct DesktopAudio<'a> {
    stream: UnixStream,
    cancel: &'a AtomicBool,
}

pub fn require_wallpaper_service() -> io::Result<()> {
    let mut stream = UnixStream::connect(socket_path()?)?;
    authenticate(&stream)?;
    request(&mut stream, WALLPAPER_SUPPORT, 0).map_err(|_| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Update the Dragon Driver desktop companion for wallpaper lighting",
        )
    })
}

pub struct DesktopWallpaper<'a> {
    stream: UnixStream,
    cancel: &'a AtomicBool,
}

impl<'a> DesktopWallpaper<'a> {
    pub fn open(cancel: &'a AtomicBool) -> io::Result<Self> {
        require_wallpaper_service()?;
        let mut stream = UnixStream::connect(socket_path()?)?;
        authenticate(&stream)?;
        request(&mut stream, WALLPAPER, 0)?;
        stream.set_read_timeout(Some(Duration::from_millis(200)))?;
        Ok(Self { stream, cancel })
    }

    pub fn next_image(&mut self) -> io::Result<Option<Vec<u8>>> {
        let mut length = [0; 4];
        self.read_exact(&mut length)?;
        let length = u32::from_le_bytes(length) as usize;
        if length > MAX_WALLPAPER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Wallpaper response is too large",
            ));
        }
        if length == 0 {
            return Ok(None);
        }
        let mut output = vec![0; length];
        self.read_exact(&mut output)?;
        Ok(Some(output))
    }

    fn read_exact(&mut self, mut output: &mut [u8]) -> io::Result<()> {
        while !output.is_empty() {
            if self.cancel.load(Ordering::Relaxed) {
                return Err(io::Error::from(io::ErrorKind::Interrupted));
            }
            match self.stream.read(output) {
                Ok(0) => {
                    return Err(io::Error::new(
                        io::ErrorKind::UnexpectedEof,
                        "Wallpaper service disconnected",
                    ));
                }
                Ok(count) => output = &mut output[count..],
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::WouldBlock
                            | io::ErrorKind::TimedOut
                            | io::ErrorKind::Interrupted
                    ) => {}
                Err(error) => return Err(error),
            }
        }
        Ok(())
    }
}

impl<'a> DesktopAudio<'a> {
    pub fn open(cancel: &'a AtomicBool) -> io::Result<Self> {
        Self::connect(cancel, AUDIO)
    }

    pub fn open_bars(cancel: &'a AtomicBool) -> io::Result<Self> {
        require_audio_service()?;
        Self::connect(cancel, AUDIO_BARS)
    }

    fn connect(cancel: &'a AtomicBool, command: u8) -> io::Result<Self> {
        let mut stream = UnixStream::connect(socket_path()?).map_err(|error| {
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
            ) {
                io::Error::new(
                    error.kind(),
                    "Set up the Dragon Driver desktop service to use desktop audio.",
                )
            } else {
                error
            }
        })?;
        authenticate(&stream)?;
        request(&mut stream, command, 0).map_err(|error| {
            if command == AUDIO_BARS && error.kind() == io::ErrorKind::NotConnected {
                io::Error::new(
                    error.kind(),
                    "Install PipeWire's pw-record tool to use audio lighting.",
                )
            } else {
                error
            }
        })?;
        stream.set_read_timeout(Some(Duration::from_millis(100)))?;
        Ok(Self { stream, cancel })
    }
}

pub fn require_audio_service() -> io::Result<()> {
    let mut stream = UnixStream::connect(socket_path()?)?;
    authenticate(&stream)?;
    request(&mut stream, AUDIO_SUPPORT, 0).map_err(|_| {
        io::Error::new(
            io::ErrorKind::PermissionDenied,
            "Update the Dragon Driver desktop companion for audio lighting.",
        )
    })
}

impl Read for DesktopAudio<'_> {
    fn read(&mut self, bytes: &mut [u8]) -> io::Result<usize> {
        if bytes.is_empty() {
            return Ok(0);
        }
        while !self.cancel.load(Ordering::Relaxed) {
            match self.stream.read(bytes) {
                Ok(0) => {
                    return Err(io::Error::other(
                        "Desktop audio stopped. Check the sound output.",
                    ));
                }
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::TimedOut
                            | io::ErrorKind::WouldBlock
                            | io::ErrorKind::Interrupted
                    ) => {}
                result => return result,
            }
        }
        Ok(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_lock_requires_a_regular_file() {
        let path =
            std::env::temp_dir().join(format!("dragon-config-lock-test-{}", std::process::id()));
        let raw = std::ffi::CString::new(path.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: the test owns the NUL-terminated path for this temporary FIFO.
        assert_eq!(unsafe { libc::mkfifo(raw.as_ptr(), 0o600) }, 0);
        let fifo = open_config_file(&path, false);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(fifo.unwrap_err().kind(), io::ErrorKind::InvalidData);

        std::os::unix::fs::symlink("/dev/null", &path).unwrap();
        let link = open_config_file(&path, true);
        std::fs::remove_file(&path).unwrap();
        assert_eq!(link.unwrap_err().raw_os_error(), Some(libc::ELOOP));

        let lock = open_config_file(&path, true).unwrap();
        lock.lock().unwrap();
        assert!(open_config_file(&path, false).unwrap().try_lock().is_err());
        drop(lock);
        assert!(open_config_file(&path, false).unwrap().try_lock().is_ok());
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn local_peer_and_protocol_round_trip() {
        let (mut left, mut right) = UnixStream::pair().unwrap();
        authenticate(&left).unwrap();
        let server = std::thread::spawn(move || {
            assert_eq!(read_request(&mut right).unwrap(), (OPEN, 123));
            let file = File::open("/dev/null").unwrap();
            response(&mut right, Ok(())).unwrap();
            let connection = fd_connection(&right).unwrap();
            connection
                .send_fd(file.as_raw_fd(), gio::Cancellable::NONE)
                .unwrap();
        });
        request(&mut left, OPEN, 123).unwrap();
        let fd = fd_connection(&left)
            .unwrap()
            .receive_fd(gio::Cancellable::NONE)
            .unwrap();
        // SAFETY: successful receive_fd returned a newly owned file descriptor.
        let file = unsafe { OwnedFd::from_raw_fd(fd) };
        assert_eq!(File::from(file).read(&mut [0]).unwrap(), 0);
        server.join().unwrap();
    }

    #[test]
    #[ignore = "requires the live K617M through a running desktop service"]
    fn broker_reads_keyboard_identity() {
        let Some(node) = std::env::var_os("DRAGON_DRIVER_LIVE_K617M") else {
            return;
        };
        let node = PathBuf::from(node);
        assert!(
            open_keyboard(&node).unwrap().is_some(),
            "the desktop service must return a descriptor"
        );
        let sdk = crate::witmod_sdk::session::SdkSession::initialize_desktop().unwrap();
        let device = sdk
            .devices()
            .iter()
            .find(|device| device.node() == node)
            .unwrap();
        let identity = sdk.query_identity(device).unwrap();
        assert_eq!(identity.layout_code(), b"7153_AUSHERGB");
    }

    #[test]
    fn path_and_request_are_constrained() {
        assert_eq!(index(Path::new("/dev/hidraw42")), Some(42));
        assert!(index(Path::new("/tmp/hidraw42")).is_none());
        let (mut left, mut right) = UnixStream::pair().unwrap();
        left.write_all(&[VERSION, AUDIO, 1, 0, 0, 0]).unwrap();
        assert_eq!(
            read_request(&mut right).unwrap_err().kind(),
            io::ErrorKind::InvalidData
        );
    }
}
