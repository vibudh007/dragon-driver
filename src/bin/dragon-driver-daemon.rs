use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use dragon_driver::{
    keyboard::{lighting::Region, session::Keyboard},
    palettes::Palettes,
    reactive::Reactive,
    service,
    witmod_sdk::{discovery::ConfigInterface, session::SdkSession},
};
use gio::prelude::*;

const MAX_CLIENTS: usize = 8;
const SESSION_TIMEOUT: Duration = Duration::from_secs(2);
const REGISTRATIONS: [&str; 2] = ["frontend.app", "frontend.flatpak"];
const UNITS: [&str; 2] = ["dragon-driver.service", "dragon-driver.socket"];

static STOPPING: AtomicBool = AtomicBool::new(false);

extern "C" fn stop(_: libc::c_int) {
    STOPPING.store(true, Ordering::Relaxed);
}

fn handle_shutdown() -> io::Result<()> {
    for signal in [libc::SIGTERM, libc::SIGINT] {
        // SAFETY: the handler only sets a lock-free atomic flag.
        if unsafe { libc::signal(signal, stop as libc::sighandler_t) } == libc::SIG_ERR {
            return Err(io::Error::last_os_error());
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let argument = args.next().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "provide the installed app path",
        )
    })?;
    if service::uid() == 0 {
        let user = args
            .next()
            .and_then(|value| value.to_str()?.parse::<u32>().ok())
            .filter(|uid| *uid != 0);
        if argument == "--system"
            && args.next().is_none()
            && let Some(user) = user
        {
            return system_service(user);
        }
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "run the service as your desktop user",
        ));
    }
    if args.next().is_some() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected one installed app path",
        ));
    }
    let managed = (argument == "--managed").then(Managed::load).transpose()?;
    let frontends = match &managed {
        Some(install) => install.frontends.clone(),
        None => vec![PathBuf::from(argument)],
    };
    if frontends.iter().any(|frontend| !frontend.is_absolute()) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "expected an absolute installation path",
        ));
    }
    handle_shutdown()?;
    let listener = activated_listener()?;
    listener.set_nonblocking(true)?;
    let clients = Arc::new(AtomicUsize::new(0));
    service::prepare_config_lock()?;
    let shared = Arc::new(SharedKeyboards::default());
    let audio = Arc::new(AtomicBool::new(false));
    let palettes = Arc::new(Mutex::new(()));
    let reactive = Arc::new(Reactive::persistent(palettes.clone()));
    if frontends.iter().any(|frontend| frontend.is_file())
        && let Err(error) = reactive.resume()
    {
        eprintln!("Dragon Driver restoring reactive lighting: {error}");
    }
    let observing = palettes.clone();
    let devices = shared.clone();
    std::thread::Builder::new()
        .name("dragon-driver-effects".into())
        .spawn(move || {
            while !STOPPING.load(Ordering::Relaxed) {
                observe_devices(&observing, &devices.restored);
                std::thread::sleep(Duration::from_secs(3));
            }
        })?;
    let mut missing = None;
    while !STOPPING.load(Ordering::Relaxed) {
        if frontends.iter().any(|frontend| frontend.is_file()) {
            missing = None;
        } else {
            if managed.is_none() {
                break;
            }
            let absent = missing.get_or_insert_with(Instant::now);
            if absent.elapsed() >= Duration::from_secs(30) {
                break;
            }
        }
        if !wait_for_connection(&listener)? {
            continue;
        }
        let (stream, _) = match listener.accept() {
            Ok(pair) => pair,
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => continue,
            Err(error) => return Err(error),
        };
        if missing.is_some() || clients.load(Ordering::Relaxed) >= MAX_CLIENTS {
            continue;
        }
        clients.fetch_add(1, Ordering::Relaxed);
        let lease = Client(clients.clone());
        let audio = audio.clone();
        let shared = shared.clone();
        let reactive = reactive.clone();
        let palettes = palettes.clone();
        let _ = std::thread::Builder::new()
            .name("dragon-driver-client".into())
            .spawn(move || {
                let _lease = lease;
                let _ = serve(stream, &audio, &palettes, &shared, &reactive);
            });
    }
    reactive.shutdown();
    if !STOPPING.load(Ordering::Relaxed)
        && let Some(managed) = managed
    {
        if let Err(error) = service::release_system_access()
            && error.kind() != io::ErrorKind::NotFound
        {
            eprintln!("Dragon Driver retiring system access: {error}");
        }
        managed.uninstall()?;
    }
    Ok(())
}

struct Managed {
    directory: PathBuf,
    units: PathBuf,
    frontends: Vec<PathBuf>,
}

impl Managed {
    fn load() -> io::Result<Self> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "missing desktop home directory",
                )
            })?;
        let directory = home.join(".local/lib/dragon-driver");
        if std::env::current_exe()? != directory.join("dragon-driver-daemon") {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not an installed desktop service",
            ));
        }
        let units = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or(home.join(".config"))
            .join("systemd/user");
        let mut frontends = Vec::new();
        for registration in REGISTRATIONS {
            match fs::read_to_string(directory.join(registration)) {
                Ok(path) => frontends.push(PathBuf::from(path.trim_end_matches('\n'))),
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                Err(error) => return Err(error),
            }
        }
        Ok(Self {
            directory,
            units,
            frontends,
        })
    }

    fn uninstall(&self) -> io::Result<()> {
        let status = Command::new("/usr/bin/systemctl")
            .args(["--user", "disable"])
            .args(UNITS)
            .status()?;
        if !status.success() {
            return Err(io::Error::other(
                "could not disable the removed app's service",
            ));
        }
        self.remove_files()?;
        let _ = Command::new("/usr/bin/systemctl")
            .args(["--user", "daemon-reload"])
            .status();
        let _ = Command::new("/usr/bin/systemctl")
            .args(["--user", "--no-block", "stop", "dragon-driver.socket"])
            .status();
        Ok(())
    }
    fn remove_files(&self) -> io::Result<()> {
        for name in UNITS {
            let path = self.units.join(name);
            if fs::read_to_string(&path)
                .is_ok_and(|text| text.starts_with("# Managed by Dragon Driver\n"))
            {
                fs::remove_file(path)?;
            }
        }
        for registration in REGISTRATIONS {
            match fs::remove_file(self.directory.join(registration)) {
                Ok(()) => (),
                Err(error) if error.kind() == io::ErrorKind::NotFound => (),
                Err(error) => return Err(error),
            }
        }
        fs::remove_file(self.directory.join("dragon-driver-daemon"))?;
        let _ = fs::remove_dir(&self.directory);
        Ok(())
    }
}

fn activated_listener() -> io::Result<UnixListener> {
    let expected = std::process::id().to_string();
    if std::env::var("LISTEN_PID").ok().as_deref() != Some(expected.as_str())
        || std::env::var("LISTEN_FDS").ok().as_deref() != Some("1")
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "the service requires its systemd user socket",
        ));
    }
    const SOCKET_FD: i32 = 3;
    // SAFETY: fcntl checks the inherited systemd descriptor and makes it close on exec.
    if unsafe { libc::fcntl(SOCKET_FD, libc::F_SETFD, libc::FD_CLOEXEC) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: systemd passed this valid descriptor exclusively to this process.
    let fd = unsafe { OwnedFd::from_raw_fd(SOCKET_FD) };
    let listener = UnixListener::from(fd);
    listener.local_addr()?;
    Ok(listener)
}

fn wait_for_connection(listener: &UnixListener) -> io::Result<bool> {
    let mut descriptor = libc::pollfd {
        fd: listener.as_raw_fd(),
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: descriptor points to one pollfd and the listener stays alive.
    let ready = unsafe { libc::poll(&raw mut descriptor, 1, 1000) };
    if ready < 0 {
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
    Ok(ready > 0 && descriptor.revents & libc::POLLIN != 0)
}

struct Client(Arc<AtomicUsize>);
impl Drop for Client {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

fn system_service(user: u32) -> io::Result<()> {
    let listener = activated_listener()?;
    for stream in listener.incoming() {
        let mut stream = stream?;
        let result = (|| {
            service::authenticate_uid(&stream, user)?;
            stream.set_read_timeout(Some(SESSION_TIMEOUT))?;
            stream.set_write_timeout(Some(SESSION_TIMEOUT))?;
            let (operation, index) = service::read_request(&mut stream)?;
            if operation == service::RELEASE_SYSTEM_ACCESS {
                service::response(&mut stream, Ok(()))?;
                let unit = format!("dragon-driver-access@{user}.socket");
                let status = Command::new("/usr/bin/systemctl")
                    .args(["--no-block", "disable", "--now", &unit])
                    .status()?;
                if !status.success() {
                    return Err(io::Error::other(
                        "Could not retire the system access socket",
                    ));
                }
                return Ok(());
            }
            if operation != service::OPEN {
                return service::response(
                    &mut stream,
                    Err(io::Error::from(io::ErrorKind::PermissionDenied)),
                );
            }
            send_keyboard(&mut stream, open_local_keyboard(index))
        })();
        if let Err(error) = result {
            eprintln!("Dragon Driver access request: {error}");
        }
    }
    Ok(())
}

fn send_keyboard(stream: &mut UnixStream, file: io::Result<File>) -> io::Result<()> {
    match file {
        Ok(file) => {
            service::response(stream, Ok(()))?;
            service::fd_connection(stream)?
                .send_fd(file.as_raw_fd(), gio::Cancellable::NONE)
                .map_err(io::Error::other)
        }
        Err(error) => service::response(stream, Err(error)),
    }
}

fn serve(
    mut stream: UnixStream,
    audio: &AtomicBool,
    palettes: &Mutex<()>,
    shared: &SharedKeyboards,
    reactive: &Arc<Reactive>,
) -> io::Result<()> {
    stream.set_nonblocking(false)?;
    service::authenticate(&stream)?;
    stream.set_read_timeout(Some(SESSION_TIMEOUT))?;
    stream.set_write_timeout(Some(SESSION_TIMEOUT))?;
    match service::read_request(&mut stream) {
        Ok((service::OPEN, index)) => {
            let file = (|| {
                let sdk = SdkSession::initialize_system_broker()?;
                let interface = configuration_interface(&sdk, index)?;
                shared.open_restored(&sdk, interface)
            })();
            send_keyboard(&mut stream, file)
        }
        Ok((service::OPEN_SHARED, index)) => shared.serve(stream, index),
        Ok((service::REACTIVE, length)) => {
            let result = service::read_reactive(&mut stream, length)
                .and_then(|(target, request)| reactive.handle(target, request));
            match result {
                Ok(status) => service::write_reactive(&mut stream, status),
                Err(error) => service::response(&mut stream, Err(error)),
            }
        }
        Ok((service::AUDIO, _)) => capture(stream, audio, false),
        Ok((service::AUDIO_BARS, _)) => capture(stream, audio, true),
        Ok((service::AUDIO_SUPPORT, _)) => service::response(&mut stream, Ok(())),
        Ok((service::WALLPAPER, _)) => dragon_driver::wallpaper_source::serve(stream),
        Ok((service::WALLPAPER_SUPPORT_V1..=service::WALLPAPER_SUPPORT, _)) => {
            service::response(&mut stream, Ok(()))
        }
        Ok((service::PING | service::LIGHTING_PERSISTENCE_SUPPORT, _)) => {
            service::response(&mut stream, Ok(()))
        }
        Ok((service::PALETTE_READ, _)) => match Palettes::open().and_then(|store| store.snapshot())
        {
            Ok(snapshot) => {
                service::response(&mut stream, Ok(()))?;
                stream.write_all(&(snapshot.len() as u32).to_le_bytes())?;
                stream.write_all(snapshot.as_bytes())
            }
            Err(error) => service::response(&mut stream, Err(error)),
        },
        Ok((service::PALETTE, length)) => {
            let result = (|| {
                let (model, firmware, update) = service::read_palette_update(&mut stream, length)?;
                let _guard = palettes
                    .lock()
                    .map_err(|_| io::Error::other("Palette store unavailable"))?;
                let mut store = Palettes::open()?;
                match update {
                    service::PaletteUpdate::Slot { slot, frame } => {
                        store.set_slot(&model, &firmware, slot, frame.map(|frame| *frame))
                    }
                    service::PaletteUpdate::Effect(mode) => {
                        store.set_effect(&model, &firmware, mode)
                    }
                    service::PaletteUpdate::Reset => store.clear_slots(&model, &firmware),
                    service::PaletteUpdate::Lighting { region, effect } => {
                        store.set_lighting(&model, &firmware, region, effect)
                    }
                    service::PaletteUpdate::ConfirmedSlot {
                        slot,
                        frame,
                        effect,
                    } => store.confirm_slot(&model, &firmware, slot, *frame, effect),
                }
            })();
            if let Err(error) = &result {
                eprintln!("Dragon Driver palette update: {error}");
            }
            service::response(&mut stream, result)
        }
        Err(error) => service::response(&mut stream, Err(error)),
        Ok(_) => unreachable!("the protocol parser checks the operation"),
    }
}

#[derive(Default)]
struct SharedKeyboards {
    leases: Mutex<HashMap<u32, (ConfigInterface, Weak<File>)>>,
    restored: Restored,
}

#[derive(Default)]
struct Restored(Mutex<HashMap<PathBuf, ConfigInterface>>);

impl Restored {
    fn once(
        &self,
        interface: &ConfigInterface,
        restore: impl FnOnce() -> io::Result<()>,
    ) -> io::Result<()> {
        let mut restored = self
            .0
            .lock()
            .map_err(|_| io::Error::other("Lighting restore state is unavailable"))?;
        if restored.get(interface.node()) != Some(interface) {
            restore()?;
            restored.insert(interface.node().to_owned(), interface.clone());
        }
        Ok(())
    }

    fn retain_connected(&self, devices: &[ConfigInterface]) {
        if let Ok(mut restored) = self.0.lock() {
            restored.retain(|_, device| devices.contains(device));
        }
    }
}

fn configuration_interface(sdk: &SdkSession, index: u32) -> io::Result<&ConfigInterface> {
    let path = PathBuf::from(format!("/dev/hidraw{index}"));
    sdk.devices()
        .iter()
        .find(|device| device.node() == path)
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))
}

impl SharedKeyboards {
    fn serve(&self, mut stream: UnixStream, index: u32) -> io::Result<()> {
        let file = match self.open(index) {
            Ok(file) => file,
            Err(error) => return service::response(&mut stream, Err(error)),
        };
        service::response(&mut stream, Ok(()))?;
        service::fd_connection(&stream)?
            .send_fd(file.as_raw_fd(), gio::Cancellable::NONE)
            .map_err(io::Error::other)?;
        // The socket is the membership lease. Keep the broker descriptor until the peer closes.
        let mut peer = libc::pollfd {
            fd: stream.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        loop {
            // SAFETY: peer borrows a live socket descriptor and is valid throughout poll.
            if unsafe { libc::poll(&raw mut peer, 1, -1) } >= 0 {
                return Ok(());
            }
            let error = io::Error::last_os_error();
            if error.kind() != io::ErrorKind::Interrupted {
                return Err(error);
            }
        }
    }

    fn open(&self, index: u32) -> io::Result<Arc<File>> {
        let sdk = SdkSession::initialize_system_broker()?;
        let interface = configuration_interface(&sdk, index)?;
        let mut devices = self
            .leases
            .lock()
            .map_err(|error| io::Error::other(error.to_string()))?;
        devices.retain(|_, (_, file)| file.strong_count() > 0);
        if let Some((existing, file)) = devices.get(&index)
            && existing == interface
            && let Some(file) = file.upgrade()
        {
            return Ok(file);
        }
        let file = Arc::new(self.open_restored(&sdk, interface)?);
        devices.insert(index, (interface.clone(), Arc::downgrade(&file)));
        Ok(file)
    }

    fn open_restored(&self, sdk: &SdkSession, interface: &ConfigInterface) -> io::Result<File> {
        let file = service::open_system_keyboard(interface.node())?
            .ok_or_else(|| io::Error::from(io::ErrorKind::PermissionDenied))?;
        self.restored.once(interface, || {
            let store = Palettes::open()?;
            let mut keyboard = Keyboard::open_broker_file(sdk, interface, file.try_clone()?)?;
            store.restore(&mut keyboard)
        })?;
        Ok(file)
    }
}

fn open_local_keyboard(index: u32) -> io::Result<File> {
    let node = format!("/dev/hidraw{index}");
    let sdk = SdkSession::initialize()?;
    let interface = sdk
        .devices()
        .iter()
        .find(|interface| interface.node() == Path::new(&node))
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "the device is not a Redragon configuration interface",
            )
        })?;
    let file = sdk.open(interface)?.into_file()?;
    // SAFETY: the descriptor is owned by file. The lease follows the open description
    // through descriptor passing and is released when the final recipient closes it.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == -1 {
        let error = io::Error::last_os_error();
        return Err(if error.kind() == io::ErrorKind::WouldBlock {
            io::Error::from(io::ErrorKind::ResourceBusy)
        } else {
            error
        });
    }
    Ok(file)
}

fn observe_devices(palettes: &Mutex<()>, restored: &Restored) {
    let Ok(sdk) = SdkSession::initialize_system_broker() else {
        return;
    };
    restored.retain_connected(sdk.devices());
    for device in sdk.devices() {
        let Ok(mut keyboard) = Keyboard::open(&sdk, device) else {
            continue;
        };
        let result = restored.once(device, || Palettes::open()?.restore(&mut keyboard));
        if let Err(error) = result {
            eprintln!("Dragon Driver restoring lighting: {error}");
            continue;
        }
        let model = keyboard.layout().family;
        let firmware = String::from_utf8_lossy(keyboard.identity().firmware_version()).into_owned();
        for region in [Region::Keyboard, Region::Side] {
            if region == Region::Side && !keyboard.layout().side_lighting {
                continue;
            }
            let Ok(effect) = keyboard.read_lighting(region) else {
                continue;
            };
            if let Ok(_guard) = palettes.lock() {
                let result = Palettes::open()
                    .and_then(|mut store| store.set_lighting(model, &firmware, region, effect));
                if let Err(error) = result {
                    eprintln!("Dragon Driver reading effect: {error}");
                }
            }
        }
    }
}

struct AudioLease<'a>(&'a AtomicBool);
impl Drop for AudioLease<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}

fn capture(mut stream: UnixStream, audio: &AtomicBool, bars: bool) -> io::Result<()> {
    if audio.swap(true, Ordering::AcqRel) {
        return service::response(
            &mut stream,
            Err(io::Error::from(io::ErrorKind::ResourceBusy)),
        );
    }
    let _lease = AudioLease(audio);
    let mut recorder = match Recorder::open() {
        Ok(recorder) => recorder,
        Err(error) => return service::response(&mut stream, Err(error)),
    };
    service::response(&mut stream, Ok(()))?;
    let mut visualizer = bars.then(dragon_driver::keyboard::audio::Bars::default);
    relay_audio(
        &mut stream,
        &mut recorder.pcm,
        |stream, pcm| match &mut visualizer {
            Some(bars) => bars.push(pcm, |levels| stream.write_all(levels)),
            None => stream.write_all(pcm),
        },
    )
}

struct Recorder {
    child: Child,
    pcm: ChildStdout,
}

impl Recorder {
    fn open() -> io::Result<Self> {
        let rate = dragon_driver::keyboard::audio::CAPTURE_RATE.to_string();
        let mut child = Command::new("/usr/bin/pw-record")
            .args([
                "--raw",
                "--format=s16",
                "--rate", &rate,
                "--channels=1",
                "--channel-map=mono",
                "--latency=10ms",
                "--properties={ stream.capture.sink = true node.name = dragon-driver-audio node.description = \"Dragon Driver audio lighting\" }",
                "-",
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()?;
        let Some(pcm) = child.stdout.take() else {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("desktop audio did not provide a stream"));
        };
        Ok(Self { child, pcm })
    }
}

impl Drop for Recorder {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn relay_audio(
    stream: &mut UnixStream,
    pcm: &mut (impl Read + AsRawFd),
    mut forward: impl FnMut(&mut UnixStream, &[u8]) -> io::Result<()>,
) -> io::Result<()> {
    let mut poll = [
        libc::pollfd {
            fd: stream.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
        libc::pollfd {
            fd: pcm.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        },
    ];
    let mut buffer = [0; 4096];
    loop {
        // After the request, the client sends nothing. Readability or hangup ends capture.
        // SAFETY: the two descriptors are borrowed from resources alive throughout poll.
        let ready = unsafe { libc::poll(poll.as_mut_ptr(), poll.len() as libc::nfds_t, -1) };
        if ready < 0 {
            let error = io::Error::last_os_error();
            if error.kind() == io::ErrorKind::Interrupted {
                continue;
            }
            return Err(error);
        }
        if poll[0].revents != 0 {
            return Ok(());
        }
        let count = pcm.read(&mut buffer)?;
        if count == 0 {
            return Ok(());
        }
        forward(stream, &buffer[..count])?;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_or_arbitrary_node_is_rejected() {
        assert_eq!(
            open_local_keyboard(u32::MAX).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn automatic_uninstall_only_removes_the_service_files_it_owns() {
        let root =
            std::env::temp_dir().join(format!("dragon-driver-cleanup-test-{}", std::process::id()));
        fs::create_dir(&root).unwrap();
        let directory = root.join("service");
        let units = root.join("units");
        fs::create_dir(&directory).unwrap();
        fs::create_dir(&units).unwrap();
        fs::write(directory.join("dragon-driver-daemon"), b"fixture").unwrap();
        fs::write(directory.join("frontend.app"), b"/tmp/example").unwrap();
        fs::write(
            units.join("dragon-driver.socket"),
            b"# Managed by Dragon Driver\n",
        )
        .unwrap();
        fs::write(units.join("dragon-driver.service"), b"# Unrelated unit").unwrap();
        Managed {
            directory: directory.clone(),
            units: units.clone(),
            frontends: Vec::new(),
        }
        .remove_files()
        .unwrap();
        assert!(!directory.exists());
        assert!(!units.join("dragon-driver.socket").exists());
        assert!(units.join("dragon-driver.service").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn audio_relay_forwards_data_and_stops_on_disconnect() {
        let (mut source, mut read_pcm) = UnixStream::pair().unwrap();
        let (mut user, mut service) = UnixStream::pair().unwrap();
        let thread = std::thread::spawn(move || {
            relay_audio(&mut service, &mut read_pcm, |out, bytes| {
                out.write_all(bytes)
            })
        });
        source.write_all(&[1, 2, 3, 4]).unwrap();
        let mut sample = [0; 4];
        user.read_exact(&mut sample).unwrap();
        assert_eq!(sample, [1, 2, 3, 4]);
        drop(user);
        thread.join().unwrap().unwrap();
    }
}
