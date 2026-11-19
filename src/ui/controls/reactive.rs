use std::cell::{Cell, RefCell};
use std::io;
use std::rc::Rc;
use std::time::{Duration, Instant};

use dragon_driver::keyboard::lighting::{AudioPalette, DEFAULT_AUDIO_PALETTE, RgbFrame};
use dragon_driver::service::{self, ReactiveMode, ReactiveRequest, ReactiveStatus, ReactiveTarget};
use gtk::{glib, prelude::*};

use super::super::{
    color_wheel::ColorWheel,
    field, label, lighting_trace, panel,
    readbacks::Readbacks,
    worker::{Connected, Tasks},
};

struct Controls {
    target: ReactiveTarget,
    tasks: Rc<Tasks>,
    revision: u64,
    audio: gtk::Switch,
    wallpaper: gtk::Switch,
    source: gtk::ComboBoxText,
    pickers: Vec<ColorWheel>,
    audio_status: gtk::Label,
    wallpaper_status: gtk::Label,
    audio_spinner: gtk::Spinner,
    wallpaper_spinner: gtk::Spinner,
    starting_since: Cell<Option<Instant>>,
    manual: Cell<AudioPalette>,
    displayed: RefCell<ReactiveStatus>,
    pending: RefCell<Option<ReactiveRequest>>,
    in_flight: Cell<bool>,
    updating: Cell<bool>,
    last_audio: Cell<bool>,
    last_poll: Cell<Option<Instant>>,
    preview: Box<dyn Fn(bool, Option<RgbFrame>)>,
}

pub(super) fn reactive_panels(
    device: &Connected,
    tasks: &Rc<Tasks>,
    readbacks: &Readbacks,
    preview: impl Fn(bool, Option<RgbFrame>) + 'static,
) -> Option<gtk::Box> {
    let wireless = match device.transport? {
        0 => false,
        1 => true,
        _ => return None,
    };
    let target = ReactiveTarget::new(
        &device.node,
        device.layout.family,
        &device.firmware,
        device.usb_id,
    )?;
    let group = gtk::Box::new(gtk::Orientation::Vertical, 12);
    let audio_panel = panel("Audio-reactive lighting");
    let audio = gtk::Switch::new();
    audio.set_valign(gtk::Align::Center);
    audio.update_property(&[gtk::accessible::Property::Label("React to desktop audio")]);
    audio_panel.append(&field("React to desktop audio", &audio));

    let source = gtk::ComboBoxText::new();
    source.append(Some("manual"), "Custom colors");
    source.append(Some("wallpaper"), "Follow wallpaper");
    source.set_active_id(Some("manual"));
    let mut wheels = Vec::new();
    if wireless {
        let detail = label("Uses the keyboard’s built-in colors over 2.4 GHz.", "muted");
        detail.set_wrap(true);
        audio_panel.append(&detail);
    } else {
        audio_panel.append(&field("Color palette", &source));
        let pickers = gtk::Box::new(gtk::Orientation::Vertical, 4);
        for (name, color) in ["Background", "Low", "Medium", "High"]
            .into_iter()
            .zip(DEFAULT_AUDIO_PALETTE)
        {
            let wheel = ColorWheel::new(name);
            wheel.set_rgb(color);
            pickers.append(&field(name, wheel.widget()));
            wheels.push(wheel);
        }
        audio_panel.append(&pickers);
        let hint = label(
            "Uses custom colors until a wallpaper palette is available.",
            "muted",
        );
        hint.set_wrap(true);
        hint.set_visible(false);
        audio_panel.append(&hint);
        source.connect_changed(move |source| {
            let follow = source.active_id().as_deref() == Some("wallpaper");
            pickers.set_visible(!follow);
            hint.set_visible(follow);
        });
    }
    let (audio_feedback, audio_status, audio_spinner) = feedback();
    audio_panel.append(&audio_feedback);
    group.append(&audio_panel);

    let wallpaper = gtk::Switch::new();
    wallpaper.set_valign(gtk::Align::Center);
    wallpaper.update_property(&[gtk::accessible::Property::Label(
        "Match keyboard lighting to wallpaper",
    )]);
    let (wallpaper_feedback, wallpaper_status, wallpaper_spinner) = feedback();
    if !wireless {
        let wallpaper_panel = panel("Wallpaper-reactive lighting");
        wallpaper_panel.append(&field("Match your wallpaper", &wallpaper));
        wallpaper_panel.append(&wallpaper_feedback);
        group.append(&wallpaper_panel);
    }

    group.set_visible(wireless);
    let state = Rc::new(Controls {
        target,
        tasks: tasks.clone(),
        revision: tasks.revision.get(),
        audio,
        wallpaper,
        source,
        pickers: wheels,
        audio_status,
        wallpaper_status,
        audio_spinner,
        wallpaper_spinner,
        starting_since: Cell::new(None),
        manual: Cell::new(DEFAULT_AUDIO_PALETTE),
        displayed: RefCell::new(ReactiveStatus::default()),
        pending: RefCell::new(None),
        in_flight: Cell::new(false),
        updating: Cell::new(false),
        last_audio: Cell::new(false),
        last_poll: Cell::new(None),
        preview: Box::new(preview),
    });
    for (toggle, audio) in [(&state.audio, true), (&state.wallpaper, false)] {
        let name = if audio { "audio" } else { "wallpaper" };
        for property in ["active", "state", "sensitive"] {
            toggle.connect_notify_local(Some(property), move |toggle, _| {
                lighting_trace::event(format_args!(
                    "reactive switch {name} notify={property} active={} backend={} sensitive={}",
                    toggle.is_active(),
                    toggle.state(),
                    toggle.is_sensitive()
                ));
            });
        }
        let weak = Rc::downgrade(&state);
        toggle.connect_state_set(move |toggle, enabled| {
            let Some(state) = weak.upgrade() else {
                return glib::Propagation::Proceed;
            };
            lighting_trace::event(format_args!(
                "reactive switch {name} state-set requested={enabled} active={} backend={} applying={} in-flight={} pending={:?}",
                toggle.is_active(), toggle.state(), state.updating.get(), state.in_flight.get(), state.pending.borrow()
            ));
            if state.updating.get() {
                return glib::Propagation::Proceed;
            }
            let command = if !enabled {
                ReactiveRequest::Stop
            } else if audio {
                ReactiveRequest::Start(ReactiveMode::Audio {
                    colors: state.manual.get(),
                    follow_wallpaper: !wireless
                        && state.source.active_id().as_deref() == Some("wallpaper"),
                })
            } else {
                ReactiveRequest::Start(ReactiveMode::Wallpaper)
            };
            state.audio.set_sensitive(false);
            state.wallpaper.set_sensitive(false);
            state.queue(command);
            glib::Propagation::Stop
        });
    }
    for (index, wheel) in state.pickers.iter().enumerate() {
        let weak = Rc::downgrade(&state);
        wheel.connect_color_set(move |color| {
            if let Some(state) = weak.upgrade()
                && !state.updating.get()
            {
                let mut colors = state.manual.get();
                colors[index] = color;
                state.manual.set(colors);
                if matches!(
                    state.displayed.borrow().mode,
                    Some(ReactiveMode::Audio { .. })
                ) && state.audio.is_active()
                {
                    state.queue(ReactiveRequest::AudioColors(colors));
                }
            }
        });
    }
    let widget = group.downgrade();
    readbacks.on_snapshot(move |snapshot| {
        if let Some(widget) = widget.upgrade() {
            widget.set_visible(
                wireless
                    || snapshot
                        .lighting
                        .as_ref()
                        .is_some_and(|(modes, _)| modes.modes.contains(&10)),
            );
        }
    });

    let owned = state.clone();
    group.connect_destroy(move |_| {
        let _ = &owned;
    });
    let weak = Rc::downgrade(&state);
    group.connect_map(move |_| {
        if let Some(state) = weak.upgrade() {
            state.last_poll.set(None);
            state.advance(true);
        }
    });
    let weak = Rc::downgrade(&state);
    let widget = group.downgrade();
    glib::timeout_add_local(Duration::from_millis(250), move || {
        let (Some(state), Some(widget)) = (weak.upgrade(), widget.upgrade()) else {
            return glib::ControlFlow::Break;
        };
        if state.revision != state.tasks.revision.get() {
            return glib::ControlFlow::Break;
        }
        state.advance(widget.is_mapped());
        glib::ControlFlow::Continue
    });
    Some(group)
}

impl Controls {
    fn queue(self: &Rc<Self>, command: ReactiveRequest) {
        *self.pending.borrow_mut() = Some(command);
        self.advance(false);
    }

    fn advance(self: &Rc<Self>, poll: bool) {
        if cfg!(test) || self.in_flight.get() {
            return;
        }
        if self.pending.borrow().is_none()
            && (!poll
                || self.last_poll.get().is_some_and(|last| {
                    last.elapsed() < Duration::from_millis(900)
                        && self.displayed.borrow().mode.is_none()
                }))
        {
            return;
        }
        let command = self.pending.take();
        let requested = command.is_some();
        let for_audio = match &command {
            Some(
                ReactiveRequest::Start(ReactiveMode::Audio { .. })
                | ReactiveRequest::AudioColors(_),
            ) => true,
            Some(ReactiveRequest::Start(ReactiveMode::Wallpaper)) => false,
            _ => self.last_audio.get(),
        };
        let target = self.target.clone();
        let weak = Rc::downgrade(self);
        // Keep ownership of the pending command until the worker accepts it.
        let job = command.clone().unwrap_or(ReactiveRequest::Status);
        if self.tasks.background_device(
            move |_| {
                lighting_trace::event(format_args!("reactive request {job:?}"));
                let reply = service::reactive(&target, job);
                match &reply {
                    Ok(status) => lighting_trace::event(format_args!(
                        "reactive result mode={:?} starting={} stopping={} message={:?}",
                        status.mode, status.starting, status.stopping, status.message
                    )),
                    Err(error) => {
                        lighting_trace::event(format_args!("reactive result error={error}"))
                    }
                }
                reply
            },
            move |result| {
                if let Some(state) = weak.upgrade() {
                    state.in_flight.set(false);
                    match result {
                        Ok(status)
                            if !state.pending.borrow().as_ref().is_some_and(switch_command) =>
                        {
                            state.show(status, for_audio);
                        }
                        Ok(_) => {}

                        Err(error) if requested => {
                            let status = state.displayed.borrow().clone();
                            state.show(
                                ReactiveStatus {
                                    message: Some(connection_error(error)),
                                    ..status
                                },
                                for_audio,
                            );
                        }
                        Err(_) => {}
                    }
                    state.advance(false);
                }
            },
        ) {
            self.in_flight.set(true);
            self.last_poll.set(Some(Instant::now()));
        } else if requested {
            lighting_trace::event(format_args!(
                "reactive request waiting for keyboard worker: {command:?}"
            ));
            self.pending.replace(command);
        }
    }

    fn show(&self, status: ReactiveStatus, for_audio: bool) {
        let active = status.mode.is_some();
        let audio = matches!(status.mode, Some(ReactiveMode::Audio { .. }));
        let wallpaper = status.mode == Some(ReactiveMode::Wallpaper);
        if active || status.message.is_some() {
            self.last_audio.set(audio || !wallpaper && for_audio);
        }
        let previous = self.displayed.replace(status.clone());
        if status.starting && !status.stopping {
            if self.starting_since.get().is_none() {
                self.starting_since.set(Some(Instant::now()));
            }
        } else {
            self.starting_since.set(None);
        }
        let loading = self
            .starting_since
            .get()
            .is_some_and(|start| start.elapsed() >= Duration::from_millis(500));
        let show_audio = audio || !wallpaper && for_audio;
        self.audio_spinner.set_spinning(show_audio && loading);
        self.audio_spinner.set_visible(show_audio && loading);
        self.wallpaper_spinner.set_spinning(!show_audio && loading);
        self.wallpaper_spinner.set_visible(!show_audio && loading);
        self.updating.set(true);
        if let Some(ReactiveMode::Audio {
            colors,
            follow_wallpaper,
        }) = status.mode
            && !matches!(previous.mode, Some(ReactiveMode::Audio { .. }))
        {
            self.manual.set(colors);
            self.source.set_active_id(Some(if follow_wallpaper {
                "wallpaper"
            } else {
                "manual"
            }));
            for (wheel, color) in self.pickers.iter().zip(colors) {
                wheel.set_rgb(color);
            }
        }
        sync_switch(&self.audio, audio && !status.stopping);
        sync_switch(&self.wallpaper, wallpaper && !status.stopping);
        self.audio.set_sensitive(!wallpaper && !status.stopping);
        self.wallpaper.set_sensitive(!audio && !status.stopping);
        self.source.set_sensitive(!audio);
        self.updating.set(false);
        if previous.mode.is_some() != active || previous.frame != status.frame {
            (self.preview)(active, status.frame.map(|frame| *frame));
        }
        let message = if status.stopping {
            "Restoring your lighting…"
        } else {
            status.message.as_deref().unwrap_or("")
        };
        if show_audio {
            self.audio_status.set_label(message);
            self.wallpaper_status.set_label("");
        } else {
            self.wallpaper_status.set_label(message);
            self.audio_status.set_label("");
        }
    }
}

fn switch_command(command: &ReactiveRequest) -> bool {
    matches!(command, ReactiveRequest::Start(_) | ReactiveRequest::Stop)
}

pub(in crate::ui) fn sync_switch(toggle: &gtk::Switch, active: bool) {
    // Even assigning the current active value cancels GtkSwitch's animation.
    if toggle.state() != active {
        toggle.set_state(active);
    }
    if toggle.is_active() != active {
        toggle.set_active(active);
    }
}

fn feedback() -> (gtk::Box, gtk::Label, gtk::Spinner) {
    let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let spinner = gtk::Spinner::new();
    spinner.set_visible(false);
    let status = label("", "muted");
    status.set_wrap(true);
    row.append(&spinner);
    row.append(&status);
    (row, status, spinner)
}

fn connection_error(error: io::Error) -> String {
    match error.kind() {
        io::ErrorKind::ResourceBusy => "Turn off the other reactive lighting effect first.".into(),
        _ => format!("Couldn’t change reactive lighting. {error}"),
    }
}
