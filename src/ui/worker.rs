use std::cell::{Cell, RefCell};
use std::io;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread;
use std::time::{Duration, Instant};

use dragon_driver::keyboard::{
    lighting::{Effect, Region, RgbFrame},
    models::Layout,
    session::Keyboard,
};
use dragon_driver::witmod_sdk::keymap::{KEY_COLUMNS, KEY_ROWS};
use dragon_driver::witmod_sdk::{catalog::UsbId, discovery::ConfigInterface, session::SdkSession};
use gtk::{glib, prelude::*};

use super::desktop::{self, Desktop, Event};
use super::lighting_trace;
use dragon_driver::keyboard::{
    bindings::ProfilePlan, desktop::DesktopBinding, session::UploadResult,
};
use dragon_driver::{
    palettes::{Palettes, write_keyboard_palette},
    service::{self, PaletteUpdate, ReactiveRequest, ReactiveTarget},
};

type Job = Box<dyn FnOnce(&mut Device) + Send>;

#[derive(Clone)]
pub(super) struct Candidate {
    pub interface: ConfigInterface,
    pub name: String,
}

pub(super) struct Connected {
    pub layout: &'static Layout,
    pub name: String,
    pub firmware: String,
    pub node: PathBuf,
    pub usb_id: UsbId,
    pub transport: Option<u8>,
    pub palettes: [Option<RgbFrame>; 2],
    pub palette_slot: u8,
    pub palette_error: Option<String>,
}

impl Connected {
    pub fn gamepad(&self) -> bool {
        self.usb_id.vendor == 0x0416 && (0x7472..=0x7474).contains(&self.usb_id.product)
    }
}

pub(super) struct Device {
    keyboard: Option<Keyboard>,
    palette_key: Option<(&'static str, String)>,
    reactive_target: Option<ReactiveTarget>,
    palettes: io::Result<Palettes>,
    last_effect: [Option<Effect>; 2],
    desktop: Desktop,
    revision: u64,
    events: mpsc::SyncSender<(u64, Event)>,
    pending_focus: Option<Event>,
    last_action: Option<(DesktopBinding, Instant)>,
}

impl Device {
    pub fn scan(&mut self) -> io::Result<Vec<Candidate>> {
        self.reset();
        if !cfg!(test) {
            service::require_user_service()?;
            service::require_lighting_persistence()?;
        }
        let sdk = SdkSession::initialize()?;
        Ok(sdk
            .devices()
            .iter()
            .cloned()
            .map(|interface| {
                let metadata = interface.usb_metadata().ok();
                let product = metadata.and_then(|data| data.product);
                let name = product
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or_else(|| String::from("USB keyboard"));
                Candidate { interface, name }
            })
            .collect())
    }

    pub fn connect(&mut self, candidate: Candidate, revision: u64) -> io::Result<Connected> {
        self.reset();
        if !cfg!(test) {
            service::require_user_service()?;
        }
        let sdk = if cfg!(test) {
            SdkSession::initialize()?
        } else {
            SdkSession::initialize_desktop()?
        };
        if !sdk.devices().contains(&candidate.interface) {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "The keyboard was disconnected",
            ));
        }
        let mut keyboard = Keyboard::open(&sdk, &candidate.interface)?;
        let transport = match keyboard.read_link_status() {
            Ok(status) => Some(status.transport),
            Err(_) => {
                drop(keyboard);
                keyboard = Keyboard::open(&sdk, &candidate.interface)?;
                None
            }
        };
        self.revision = revision;
        let layout = keyboard.layout();
        let firmware = String::from_utf8_lossy(keyboard.identity().firmware_version()).into_owned();
        let usb_id = candidate.interface.usb_id();
        self.palette_key = Some((layout.family, firmware.clone()));
        self.reactive_target =
            ReactiveTarget::new(candidate.interface.node(), layout.family, &firmware, usb_id);
        self.palettes = Palettes::open_frontend();
        self.keyboard = Some(keyboard);
        let palette_error = if cfg!(test) || self.reactive_active() {
            None
        } else {
            self.initialize_palette(layout.family)
                .err()
                .map(|error| format!("Couldn’t initialize saved key colors: {error}"))
        };
        let name = if layout.family == "7153_AUSHERGB" {
            "K617 Fizz Rapid Trigger".into()
        } else {
            candidate.name
        };
        let palettes = self.saved_palettes();
        let palette_slot = self
            .palettes
            .as_ref()
            .map_or(0, |palettes| palettes.last_slot(layout.family, &firmware));
        let connected = Connected {
            layout,
            name,
            firmware,
            node: candidate.interface.node().into(),
            usb_id,
            transport,
            palettes,
            palette_slot,
            palette_error,
        };
        Ok(connected)
    }

    fn reset(&mut self) {
        self.keyboard = None;
        self.palette_key = None;
        self.reactive_target = None;
        self.last_effect = [None; 2];
        let _ = self.desktop.watch(false);
        self.last_action = None;
        self.pending_focus = None;
    }

    fn palette_store_error(error: &io::Error) -> io::Error {
        io::Error::new(error.kind(), format!("Couldn’t open saved colors: {error}"))
    }

    pub(super) fn saved_palettes(&self) -> [Option<RgbFrame>; 2] {
        match (&self.palette_key, &self.palettes) {
            (Some((model, firmware)), Ok(palettes)) => palettes.slots(model, firmware),
            _ => [None; 2],
        }
    }

    pub(super) fn restore_defaults(&mut self, magnetic: bool) -> io::Result<()> {
        if magnetic {
            return self.keyboard()?.reset_magnetic_settings();
        }
        if self.palette_key.is_some() {
            self.save_palette_update(PaletteUpdate::Reset)?;
        }
        self.keyboard()?.reset_settings()
    }

    fn save_palette_update(&mut self, update: PaletteUpdate) -> io::Result<()> {
        let (model, firmware) = self.palette_key.as_ref().ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotConnected, "Connect a keyboard first")
        })?;
        service::update_palette(model, firmware, update)?;
        self.palettes = Palettes::open_frontend();
        Ok(())
    }

    fn initialize_palette(&mut self, model: &str) -> io::Result<()> {
        let palettes = self.palettes.as_ref().map_err(Self::palette_store_error)?;
        if !palettes.needs_initialization(model) {
            return Ok(());
        }
        let keyboard = self.keyboard()?;
        keyboard.recover_reads()?;
        if !keyboard
            .read_lighting_modes(Region::Keyboard)?
            .modes
            .contains(&10)
        {
            return Ok(());
        }
        let brightness = keyboard.read_lighting(Region::Keyboard)?.brightness;
        self.apply_palette(0, brightness, &[[[255, 0, 0]; KEY_COLUMNS]; KEY_ROWS])?;
        Ok(())
    }

    pub(super) fn apply_palette(
        &mut self,
        slot: u8,
        brightness: u16,
        frame: &RgbFrame,
    ) -> io::Result<Effect> {
        lighting_trace::event(format_args!(
            "write custom Keyboard: slot={slot} brightness={brightness}"
        ));
        let result = write_keyboard_palette(self.keyboard()?, slot, brightness, frame);
        self.palettes = Palettes::open_frontend();
        result
    }

    pub(super) fn reactive_active(&self) -> bool {
        self.reactive_target.as_ref().is_some_and(|target| {
            service::reactive(target, ReactiveRequest::Status)
                .is_ok_and(|status| status.mode.is_some())
        })
    }

    pub(super) fn persist_effect(&mut self, region: Region, effect: Effect) -> io::Result<()> {
        if cfg!(test) {
            return Ok(());
        }
        let (model, firmware) = self
            .palette_key
            .as_ref()
            .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?;
        service::update_palette(model, firmware, PaletteUpdate::Lighting { region, effect })?;
        self.last_effect[region.index()] = Some(effect);
        Ok(())
    }

    pub(super) fn observe_effect(&mut self, region: Region, effect: Effect) {
        if !cfg!(test)
            && self.last_effect[region.index()] != Some(effect)
            && !self.reactive_active()
            && let Err(error) = self.persist_effect(region, effect)
        {
            lighting_trace::event(format_args!("couldn’t save active effect: {error}"));
        }
    }

    pub fn watch_focus(&mut self, enabled: bool) -> io::Result<()> {
        self.pending_focus = None;
        self.desktop.watch(enabled)
    }

    pub fn still_focused(&self, target: u32) -> bool {
        self.desktop.still_focused(target)
    }

    fn run(&mut self, job: Job) {
        let _guard = if cfg!(test) {
            None
        } else {
            match service::config_guard() {
                Ok(guard) => Some(guard),
                Err(_) => {
                    // A stopped companion may be restarted by a scan.
                    self.reset();
                    None
                }
            }
        };
        job(self);
        if let Some(keyboard) = &mut self.keyboard
            && keyboard.has_desktop_bindings()
            && let Err(error) = keyboard.discard_pending_bindings()
        {
            keyboard.clear_desktop_bindings();
            let _ = self
                .events
                .try_send((self.revision, Event::Message(error.to_string())));
        }
    }

    pub fn upload(&mut self, plan: &ProfilePlan) -> io::Result<UploadResult> {
        self.last_action = None;
        self.keyboard()?.upload_profile(plan)
    }

    fn interested(&self) -> bool {
        self.desktop.watching()
            || self
                .keyboard
                .as_ref()
                .is_some_and(Keyboard::has_desktop_bindings)
    }

    fn tick(&mut self) {
        let now = Instant::now();
        if let Some(focus) = self.desktop.poll(now) {
            self.pending_focus = Some(focus);
        }
        if let Some(focus) = self.pending_focus.take()
            && let Err(mpsc::TrySendError::Full((_, focus))) =
                self.events.try_send((self.revision, focus))
        {
            self.pending_focus = Some(focus);
        }
        let Some(keyboard) = self
            .keyboard
            .as_mut()
            .filter(|keyboard| keyboard.has_desktop_bindings())
        else {
            return;
        };
        let _guard = if cfg!(test) {
            None
        } else if let Ok(guard) = service::config_guard() {
            Some(guard)
        } else {
            return;
        };
        match keyboard.poll_desktop_binding() {
            Ok(Some(action)) => {
                if self.last_action.as_ref().is_some_and(|(last, time)| {
                    last == &action && now.duration_since(*time) < Duration::from_millis(160)
                }) {
                    return;
                }
                let target = matches!(action, DesktopBinding::Text(_))
                    .then(|| self.desktop.paste_target())
                    .flatten();
                self.last_action = Some((action.clone(), now));
                let _ = self
                    .events
                    .try_send((self.revision, Event::Binding(action, target)));
            }
            Err(error) => {
                keyboard.clear_desktop_bindings();
                let _ = self
                    .events
                    .try_send((self.revision, Event::Message(error.to_string())));
            }
            Ok(None) => (),
        }
    }

    pub fn keyboard(&mut self) -> io::Result<&mut Keyboard> {
        self.keyboard
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotConnected, "Connect a keyboard first"))
    }
}

pub(super) struct Tasks {
    sender: mpsc::Sender<Job>,
    events: mpsc::Receiver<(u64, Event)>,
    pub busy: Cell<bool>,
    reading: Cell<bool>,
    pub tracking: Cell<bool>,
    pub revision: Cell<u64>,
    pub cancel: Arc<AtomicBool>,
    status: gtk::Label,
    spinner: gtk::Spinner,
    controls: gtk::Widget,
    pub stop: gtk::Button,
    copy_text: gtk::Button,
    pending_text: RefCell<Option<String>>,
}

impl Tasks {
    pub fn new(
        status: &gtk::Label,
        spinner: &gtk::Spinner,
        controls: &impl IsA<gtk::Widget>,
        stop: &gtk::Button,
        copy_text: &gtk::Button,
    ) -> io::Result<Rc<Self>> {
        let (sender, receiver) = mpsc::channel::<Job>();
        let (outbound, events) = mpsc::sync_channel(12);
        let native_x11 = desktop::native_x11();
        thread::Builder::new()
            .name("keyboard-io".into())
            .spawn(move || {
                let mut device = Device {
                    keyboard: None,
                    palette_key: None,
                    reactive_target: None,
                    last_effect: [None; 2],
                    palettes: Err(io::Error::from(io::ErrorKind::NotConnected)),
                    desktop: Desktop::new(native_x11),
                    revision: 0,
                    events: outbound,
                    last_action: None,
                    pending_focus: None,
                };
                loop {
                    if device.interested() {
                        match receiver.recv_timeout(Duration::from_millis(35)) {
                            Ok(job) => device.run(job),
                            Err(mpsc::RecvTimeoutError::Timeout) => device.tick(),
                            Err(mpsc::RecvTimeoutError::Disconnected) => break,
                        }
                    } else {
                        let Ok(job) = receiver.recv() else { break };
                        device.run(job);
                    }
                }
            })?;
        let tasks = Rc::new(Self {
            sender,
            events,
            busy: Cell::new(false),
            reading: Cell::new(false),
            tracking: Cell::new(false),
            revision: Cell::new(0),
            cancel: Arc::new(AtomicBool::new(false)),
            status: status.clone(),
            spinner: spinner.clone(),
            controls: controls.clone().upcast(),
            stop: stop.clone(),
            copy_text: copy_text.clone(),
            pending_text: RefCell::new(None),
        });
        let weak = Rc::downgrade(&tasks);
        copy_text.connect_clicked(move |_| {
            if let Some(tasks) = weak.upgrade()
                && let Some(text) = tasks.pending_text.borrow_mut().take()
            {
                if let Some(display) = gtk::gdk::Display::default() {
                    display.clipboard().set_text(&text);
                    tasks.message("Text copied", false);
                }
                tasks.copy_text.set_visible(false);
            }
        });
        Ok(tasks)
    }

    pub fn clear_pending_text(&self) {
        self.pending_text.borrow_mut().take();
        self.copy_text.set_visible(false);
    }

    pub fn next_event(&self) -> Option<Event> {
        while let Ok((revision, event)) = self.events.try_recv() {
            if self.revision.get() == revision {
                return Some(event);
            }
        }
        None
    }

    pub fn desktop_action(&self, action: DesktopBinding, target: Option<u64>) {
        match action {
            DesktopBinding::Text(value) => {
                if let Some(display) = gtk::gdk::Display::default() {
                    if let Some(target) = target {
                        self.clear_pending_text();
                        display.clipboard().set_text(&value);
                        display.flush();
                        self.enqueue_desktop(move |desktop| desktop.paste(target));
                    } else {
                        *self.pending_text.borrow_mut() = Some(value);
                        self.copy_text.set_visible(true);
                        self.message("Text binding is ready to copy", false);
                    }
                }
            }
            DesktopBinding::Launch(id) => self.enqueue_desktop(move |_| desktop::launch(&id)),
            DesktopBinding::Profile(_) => (),
        }
    }

    fn enqueue_desktop(&self, job: impl FnOnce(&Desktop) -> io::Result<()> + Send + 'static) {
        let revision = self.revision.get();
        let _ = self.sender.send(Box::new(move |device| {
            if revision == device.revision
                && let Err(error) = job(&device.desktop)
            {
                let _ = device
                    .events
                    .try_send((revision, Event::Message(error.to_string())));
            }
        }));
    }

    pub fn message(&self, text: &str, error: bool) {
        self.status.set_label(text);
        if error {
            self.status.add_css_class("error");
        } else {
            self.status.remove_css_class("error");
        }
    }

    pub fn error(&self, error: io::Error) {
        if error.kind() == io::ErrorKind::PermissionDenied {
            self.message("The app needs permission to use this keyboard.", true);
        } else {
            self.message(&error.to_string(), true);
        }
    }

    pub fn background_keyboard<T: Send + 'static>(
        self: &Rc<Self>,
        job: impl FnOnce(&mut Keyboard) -> io::Result<T> + Send + 'static,
        done: impl FnOnce(io::Result<T>) + 'static,
    ) -> bool {
        self.background_device(move |device| device.keyboard().and_then(job), done)
    }

    pub fn background_device<T: Send + 'static>(
        self: &Rc<Self>,
        job: impl FnOnce(&mut Device) -> io::Result<T> + Send + 'static,
        done: impl FnOnce(io::Result<T>) + 'static,
    ) -> bool {
        if self.busy.get() || self.reading.replace(true) {
            return false;
        }
        let revision = self.revision.get();
        let (send, receive) = mpsc::sync_channel(1);
        let _ = self.sender.send(Box::new(move |device| {
            let result = if device.revision == revision {
                job(device)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::NotConnected,
                    "The keyboard changed",
                ))
            };
            let _ = send.send(result);
        }));
        let start = Instant::now();
        let weak = Rc::downgrade(self);
        let mut done = Some(done);
        glib::timeout_add_local(Duration::from_millis(12), move || {
            let Some(tasks) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let result = match receive.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err(io::Error::other("The keyboard worker stopped"))
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if !tasks.busy.get() && start.elapsed() >= Duration::from_millis(500) {
                        tasks.spinner.set_visible(true);
                        tasks.spinner.start();
                    }
                    return glib::ControlFlow::Continue;
                }
            };
            tasks.reading.set(false);
            if !tasks.busy.get() {
                tasks.spinner.stop();
                tasks.spinner.set_visible(false);
            }
            if tasks.revision.get() == revision
                && let Some(done) = done.take()
            {
                done(result);
            }
            glib::ControlFlow::Break
        });
        true
    }

    pub fn start<T: Send + 'static>(
        self: &Rc<Self>,
        loading: &'static str,
        stoppable: bool,
        job: impl FnOnce(&mut Device, &AtomicBool) -> io::Result<T> + Send + 'static,
        done: impl FnOnce(io::Result<T>) + 'static,
    ) {
        self.run(loading, stoppable, true, job, done);
    }

    pub fn track<T: Send + 'static>(
        self: &Rc<Self>,
        job: impl FnOnce(&mut Device, &AtomicBool) -> io::Result<T> + Send + 'static,
        done: impl FnOnce(io::Result<T>) + 'static,
    ) {
        self.tracking.set(true);
        self.run("Tracking key travel…", false, false, job, done);
    }

    fn run<T: Send + 'static>(
        self: &Rc<Self>,
        loading: &'static str,
        stoppable: bool,
        freeze_controls: bool,
        job: impl FnOnce(&mut Device, &AtomicBool) -> io::Result<T> + Send + 'static,
        done: impl FnOnce(io::Result<T>) + 'static,
    ) {
        if self.busy.replace(true) {
            return;
        }
        self.cancel.store(false, Ordering::Relaxed);
        self.controls.set_sensitive(!freeze_controls);
        self.stop.set_visible(stoppable);
        self.stop.set_sensitive(true);
        let (send, receive) = mpsc::sync_channel(1);
        let cancel = self.cancel.clone();
        if self
            .sender
            .send(Box::new(move |device| {
                let _ = send.send(job(device, &cancel));
            }))
            .is_err()
        {
            self.finish();
            done(Err(io::Error::other("The keyboard worker stopped")));
            return;
        }
        let start = Instant::now();
        let weak = Rc::downgrade(self);
        let mut done = Some(done);
        glib::timeout_add_local(Duration::from_millis(20), move || {
            let Some(tasks) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            let result = match receive.try_recv() {
                Ok(result) => result,
                Err(mpsc::TryRecvError::Disconnected) => {
                    Err(io::Error::other("The keyboard worker stopped"))
                }
                Err(mpsc::TryRecvError::Empty) => {
                    if !tasks.spinner.is_spinning() && start.elapsed() >= Duration::from_millis(500)
                    {
                        tasks.message(loading, false);
                        tasks.spinner.set_visible(true);
                        tasks.spinner.start();
                    }
                    return glib::ControlFlow::Continue;
                }
            };
            tasks.finish();
            if let Some(done) = done.take() {
                done(result);
            }
            glib::ControlFlow::Break
        });
    }

    fn finish(&self) {
        self.busy.set(false);
        self.tracking.set(false);
        self.spinner.stop();
        self.spinner.set_visible(false);
        self.controls.set_sensitive(true);
        self.stop.set_visible(false);
    }

    pub fn keyboard<T: Send + 'static>(
        self: &Rc<Self>,
        loading: &'static str,
        job: impl FnOnce(&mut Keyboard) -> io::Result<T> + Send + 'static,
        done: impl FnOnce(T) + 'static,
    ) {
        let weak = Rc::downgrade(self);
        self.start(
            loading,
            false,
            move |device, _| job(device.keyboard()?),
            move |result| {
                if let Some(tasks) = weak.upgrade() {
                    match result {
                        Ok(value) => {
                            tasks.message("", false);
                            done(value);
                        }
                        Err(error) => tasks.error(error),
                    }
                }
            },
        );
    }
}
