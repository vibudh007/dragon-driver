//! Session-owned lighting that outlives its clients.
use std::io;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, mpsc};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use crate::keyboard::{
    lighting::{AudioPalette, Region, RgbFrame},
    models::geometry,
    session::Keyboard,
};
use crate::palettes::Palettes;
use crate::service::{
    self, DesktopAudio, DesktopWallpaper, ReactiveMode, ReactiveRequest, ReactiveStatus,
    ReactiveTarget,
};
use crate::witmod_sdk::{discovery::ConfigInterface, session::SdkSession};

const KEEPALIVE: Duration = Duration::from_millis(500);

#[derive(Default)]
pub struct Reactive {
    state: Mutex<State>,
    store: Option<Arc<Mutex<()>>>,
}

#[derive(Default)]
struct State {
    current: Option<Arc<Run>>,
    shutting_down: bool,
}

struct Run {
    target: ReactiveTarget,
    cancel: AtomicBool,
    status: Mutex<ReactiveStatus>,
    finished: Condvar,
}

impl Reactive {
    pub fn persistent(store: Arc<Mutex<()>>) -> Self {
        Self {
            store: Some(store),
            ..Self::default()
        }
    }

    pub fn resume(&self) -> io::Result<()> {
        let preference = Palettes::open()?.saved_reactive().cloned();
        if let Some((target, mode)) = preference {
            self.handle(target, ReactiveRequest::Start(mode))?;
        }
        Ok(())
    }

    fn remember(&self, preference: Option<(ReactiveTarget, ReactiveMode)>) -> io::Result<()> {
        if let Some(store) = &self.store {
            let _guard = store
                .lock()
                .map_err(|_| io::Error::other("Lighting store is unavailable"))?;
            Palettes::open()?.set_reactive(preference)?;
        }
        Ok(())
    }

    pub fn handle(
        &self,
        target: ReactiveTarget,
        request: ReactiveRequest,
    ) -> io::Result<ReactiveStatus> {
        self.handle_with(target, request, |run, mode| run.render(mode))
    }

    fn handle_with(
        &self,
        target: ReactiveTarget,
        request: ReactiveRequest,
        render: impl FnOnce(&Run, ReactiveMode) -> io::Result<()> + Send + 'static,
    ) -> io::Result<ReactiveStatus> {
        if target.model.is_empty() || target.model.len() > 128 || target.firmware.len() > 128 {
            return Err(io::Error::from(io::ErrorKind::InvalidInput));
        }
        let mut state = self
            .state
            .lock()
            .map_err(|error| io::Error::other(error.to_string()))?;
        if state.shutting_down && matches!(request, ReactiveRequest::Start(_)) {
            return Err(io::Error::from(io::ErrorKind::NotConnected));
        }
        let current = state.current.as_ref();
        if let Some(run) = current {
            let mut status = run
                .status
                .lock()
                .map_err(|error| io::Error::other(error.to_string()))?;
            if run.target.same_model(&target) {
                match request {
                    ReactiveRequest::Status => return Ok(status.clone()),
                    ReactiveRequest::Stop if status.mode.is_some() => {
                        self.remember(None)?;
                        status.stopping = true;
                        run.cancel.store(true, Ordering::Relaxed);
                        return Ok(status.clone());
                    }
                    ReactiveRequest::AudioColors(colors) => {
                        if let Some(ReactiveMode::Audio {
                            follow_wallpaper, ..
                        }) = status.mode
                            && !status.stopping
                        {
                            let mode = ReactiveMode::Audio {
                                colors,
                                follow_wallpaper,
                            };
                            self.remember(Some((run.target.clone(), mode)))?;
                            status.mode = Some(mode);
                        }
                        return Ok(status.clone());
                    }
                    ReactiveRequest::Start(mode)
                        if !status.stopping && status.mode == Some(mode) =>
                    {
                        return Ok(status.clone());
                    }
                    _ => {}
                }
            }
            if matches!(request, ReactiveRequest::Start(_)) && status.mode.is_some() {
                return Err(io::Error::from(io::ErrorKind::ResourceBusy));
            }
        }
        let ReactiveRequest::Start(mode) = request else {
            return Ok(ReactiveStatus::default());
        };
        let status = ReactiveStatus {
            mode: Some(mode),
            starting: true,
            message: Some("Starting reactive lighting…".into()),
            ..ReactiveStatus::default()
        };
        let run = Arc::new(Run {
            target,
            cancel: AtomicBool::new(false),
            status: Mutex::new(status.clone()),
            finished: Condvar::new(),
        });
        let worker = run.clone();
        // Do not let the renderer touch the keyboard until the preference is saved.
        let (release, start) = mpsc::sync_channel(1);
        thread::Builder::new()
            .name("reactive-lighting".into())
            .spawn(move || {
                if start.recv().is_ok() {
                    worker.finish(render(&worker, mode));
                }
            })?;
        self.remember(Some((run.target.clone(), mode)))?;
        state.current = Some(run);
        let _ = release.send(());

        Ok(status)
    }

    pub fn shutdown(&self) {
        let run = self.state.lock().ok().and_then(|mut state| {
            state.shutting_down = true;
            state.current.clone()
        });
        if let Some(run) = run {
            run.cancel.store(true, Ordering::Relaxed);
            if let Ok(status) = run.status.lock() {
                let _ = run
                    .finished
                    .wait_timeout_while(status, Duration::from_secs(4), |status| {
                        status.mode.is_some()
                    });
            }
        }
    }
}

impl Run {
    fn finish(&self, result: io::Result<()>) {
        let message = if self.cancelled() {
            None
        } else {
            result.err().map(|error| {
                eprintln!("Dragon Driver reactive lighting: {error}");
                error.to_string()
            })
        };
        *self.status.lock().unwrap() = ReactiveStatus {
            message,
            ..ReactiveStatus::default()
        };
        self.finished.notify_all();
    }

    fn render(&self, mode: ReactiveMode) -> io::Result<()> {
        let mut previous = None;
        while !self.cancelled() {
            let result: io::Result<()> = self.open_keyboard(mode).and_then(|keyboard| {
                let Some((mut keyboard, interface)) = keyboard else {
                    return Err(io::Error::new(
                        io::ErrorKind::NotConnected,
                        "Waiting for your keyboard to reconnect.",
                    ));
                };
                let result = match mode {
                    ReactiveMode::Wallpaper => self.wallpaper(&mut keyboard),
                    ReactiveMode::Audio {
                        colors,
                        follow_wallpaper,
                    } => self.audio(&mut keyboard, colors, follow_wallpaper),
                };
                let present = interface.still_present().unwrap_or(false);
                if let Ok(_guard) = service::config_guard() {
                    drop(keyboard);
                }
                if present {
                    result.and(Err(io::Error::other(
                        "The lighting source stopped. Reconnecting…",
                    )))
                } else {
                    Err(io::Error::new(
                        io::ErrorKind::NotConnected,
                        "Waiting for your keyboard to reconnect.",
                    ))
                }
            });
            if self.cancelled() {
                break;
            }
            if let Err(error) = result {
                let message = error.to_string();
                if previous.as_ref() != Some(&message) {
                    eprintln!("Dragon Driver reactive lighting: {message}");
                    previous = Some(message.clone());
                }
                self.update(None, Some(&message));
            }
            for _ in 0..10 {
                if self.cancelled() {
                    break;
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        Ok(())
    }

    fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    fn update(&self, frame: Option<RgbFrame>, message: Option<&str>) {
        let mut state = self.status.lock().unwrap();
        state.starting = false;
        state.frame = frame.map(Box::new);
        state.message = message.map(str::to_owned);
    }

    fn open_keyboard(&self, mode: ReactiveMode) -> io::Result<Option<(Keyboard, ConfigInterface)>> {
        let _guard = service::config_guard()?;
        let sdk = SdkSession::initialize_shared_broker()?;
        let node = format!("/dev/hidraw{}", self.target.node);
        let candidates = sdk.devices().iter().filter(|interface| {
            let id = interface.usb_id();
            id.vendor == self.target.vendor && id.product == self.target.product
        });
        for interface in candidates
            .clone()
            .filter(|interface| interface.node().as_os_str() == node.as_str())
            .chain(candidates.filter(|interface| interface.node().as_os_str() != node.as_str()))
        {
            let mut keyboard = match Keyboard::open(&sdk, interface) {
                Ok(keyboard) => keyboard,
                Err(error)
                    if matches!(
                        error.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::NotConnected
                    ) =>
                {
                    continue;
                }
                Err(error) => return Err(error),
            };
            if keyboard.layout().family != self.target.model
                || String::from_utf8_lossy(keyboard.identity().firmware_version())
                    != self.target.firmware
            {
                continue;
            }
            let link = keyboard.read_link_status()?.transport;
            if !(link == 0 || link == 1 && matches!(mode, ReactiveMode::Audio { .. }))
                || (link == 0
                    && !keyboard
                        .read_lighting_modes(Region::Keyboard)?
                        .modes
                        .contains(&10))
            {
                return Err(io::Error::other(
                    "This keyboard connection does not support that lighting mode.",
                ));
            }
            return Ok(Some((keyboard, interface.clone())));
        }
        Ok(None)
    }

    fn wallpaper(&self, keyboard: &mut Keyboard) -> io::Result<()> {
        let geometry = geometry(keyboard.layout());
        let (wake, changed) = mpsc::sync_channel(1);
        let latest = Arc::new(Mutex::new(None));
        let inbox = latest.clone();
        let watch = Watch::new(move |result| {
            let result = result.and_then(|bytes| {
                let bytes =
                    bytes.ok_or_else(|| io::Error::other("No wallpaper image is available."))?;
                crate::wallpaper_colors::frame(&bytes, &geometry)
                    .map_err(|_| io::Error::other("Choose a PNG, JPEG, or WebP wallpaper."))
            });
            *inbox.lock().unwrap() = Some(result);
            let _ = wake.try_send(());
        })?;
        let mut shown = None;
        let mut last = Instant::now();
        let result = (|| {
            while !self.cancelled() {
                let wait = if shown.is_some() {
                    KEEPALIVE.saturating_sub(last.elapsed())
                } else {
                    KEEPALIVE
                };
                let changed = changed.recv_timeout(wait);
                if self.cancelled() {
                    break;
                }
                let pending = latest.lock().unwrap().take();
                if let Some(result) = pending {
                    match result {
                        Ok(frame) if shown != Some(frame) => {
                            let _guard = service::config_guard()?;
                            if self.cancelled() {
                                break;
                            }
                            keyboard.show_wallpaper_lighting(&frame)?;
                            shown = Some(frame);
                            last = Instant::now();
                            self.update(shown, Some("Following your wallpaper"));
                        }
                        Ok(_) => self.update(shown, Some("Following your wallpaper")),
                        Err(error) => self.update(shown, Some(&error.to_string())),
                    }
                }
                if let Some(frame) = shown
                    && last.elapsed() >= KEEPALIVE
                {
                    let _guard = service::config_guard()?;
                    if last.elapsed() >= KEEPALIVE * 2 {
                        keyboard.show_wallpaper_lighting(&frame)?;
                    } else {
                        keyboard.keep_wallpaper_lighting()?;
                    }
                    last = Instant::now();
                }
                if changed == Err(mpsc::RecvTimeoutError::Disconnected) {
                    return Err(io::Error::other("The wallpaper source stopped."));
                }
            }
            Ok(())
        })();
        let cleanup = service::config_guard().and_then(|_guard| keyboard.stop_wallpaper_lighting());
        drop(watch);
        result.and(cleanup)
    }

    fn audio(
        &self,
        keyboard: &mut Keyboard,
        fallback: AudioPalette,
        follow_wallpaper: bool,
    ) -> io::Result<()> {
        let palette = Arc::new(Mutex::new(None));
        let _watch = if follow_wallpaper {
            let output = palette.clone();
            Some(Watch::new(move |result| {
                if let Ok(Some(bytes)) = result
                    && let Ok(colors) = crate::audio_palette::extract(&bytes)
                {
                    *output.lock().unwrap() = Some(colors);
                }
            })?)
        } else {
            None
        };
        let mut bars = DesktopAudio::open_bars(&self.cancel)?;
        let mut ready = false;
        keyboard.stream_audio_bars_guarded(
            &mut bars,
            || {
                if let Some(colors) = *palette.lock().unwrap() {
                    return colors;
                }
                match self.status.lock().unwrap().mode {
                    Some(ReactiveMode::Audio { colors, .. }) => colors,
                    _ => fallback,
                }
            },
            || {
                if !ready {
                    self.update(None, Some("Listening to your current sound output"));
                    ready = true;
                }
            },
            service::config_guard,
        )
    }
}

struct Watch {
    cancel: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Watch {
    fn new(
        mut callback: impl FnMut(io::Result<Option<Vec<u8>>>) + Send + 'static,
    ) -> io::Result<Self> {
        let cancel = Arc::new(AtomicBool::new(false));
        let stop = cancel.clone();
        let thread = thread::Builder::new()
            .name("reactive-wallpaper-source".into())
            .spawn(move || {
                let result = (|| {
                    let mut source = DesktopWallpaper::open(&stop)?;
                    while !stop.load(Ordering::Relaxed) {
                        callback(Ok(source.next_image()?));
                    }
                    Ok(())
                })();
                if !stop.load(Ordering::Relaxed)
                    && let Err(error) = result
                {
                    callback(Err(error));
                }
            })?;
        Ok(Self {
            cancel,
            thread: Some(thread),
        })
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::os::unix::net::UnixStream;

    #[test]
    fn shutdown_rejects_late_starts() {
        let daemon = Reactive::default();
        daemon.shutdown();
        let result = daemon.handle_with(
            ReactiveTarget {
                node: 4,
                model: "7153_AUSHERGB".into(),
                firmware: "V1_11_27".into(),
                vendor: 0x0416,
                product: 0x7372,
            },
            ReactiveRequest::Start(ReactiveMode::Wallpaper),
            |_, _| panic!("no worker should start after shutdown"),
        );
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::NotConnected);
    }

    #[test]
    fn effect_remains_owned_by_daemon_after_start_client_disconnects() {
        let daemon = Reactive::default();
        let target = ReactiveTarget {
            node: 4,
            model: "7153_AUSHERGB".into(),
            firmware: "V1_11_27".into(),
            vendor: 0x0416,
            product: 0x7372,
        };
        let expected = [[[12, 34, 56]; 22]; 6];
        let (ready, started) = mpsc::sync_channel(1);
        let (finished, cleanup) = mpsc::sync_channel(1);

        // Submit Start over a disposable socket, then close it before rendering completes.
        let (mut client, mut server) = UnixStream::pair().unwrap();
        let request =
            ron::to_string(&(&target, ReactiveRequest::Start(ReactiveMode::Wallpaper))).unwrap();
        client.write_all(request.as_bytes()).unwrap();
        let (device, request) = service::read_reactive(&mut server, request.len() as u32).unwrap();
        let status = daemon
            .handle_with(device, request, move |run, _| {
                started.recv_timeout(Duration::from_secs(2)).unwrap();
                run.update(Some(expected), None);
                while !run.cancelled() {
                    thread::sleep(Duration::from_millis(2));
                }
                finished.send(()).unwrap();
                Ok(())
            })
            .unwrap();
        service::write_reactive(&mut server, status).unwrap();
        let mut header = [0; 5];
        client.read_exact(&mut header).unwrap();
        assert_eq!(header[0], service::OK);
        let size = u32::from_le_bytes(header[1..].try_into().unwrap());
        let reply: ReactiveStatus = service::read_reactive(&mut client, size).unwrap();
        assert_eq!(reply.mode, Some(ReactiveMode::Wallpaper));
        drop(client);
        drop(server);
        ready.send(()).unwrap();

        // A new client sees the existing mode and the rendered frame even at a new node.
        let reconnected = ReactiveTarget { node: 7, ..target };
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let status = daemon
                .handle(reconnected.clone(), ReactiveRequest::Status)
                .unwrap();
            assert_eq!(status.mode, Some(ReactiveMode::Wallpaper));
            if let Some(frame) = status.frame {
                assert_eq!(*frame, expected);
                break;
            }
            assert!(
                Instant::now() < deadline,
                "Background renderer did not publish a frame"
            );
            thread::sleep(Duration::from_millis(2));
        }
        let stop = daemon
            .handle(reconnected.clone(), ReactiveRequest::Stop)
            .unwrap();
        assert!(stop.stopping);
        cleanup.recv_timeout(Duration::from_secs(2)).unwrap();
        daemon.shutdown();
        assert_eq!(
            daemon.handle(reconnected, ReactiveRequest::Status).unwrap(),
            ReactiveStatus::default()
        );
    }
}
