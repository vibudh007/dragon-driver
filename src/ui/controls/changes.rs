use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::io;
use std::rc::Rc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use dragon_driver::keyboard::lighting::{Effect, Region, RgbFrame};
use gtk::glib;

use super::super::lighting_trace;
use super::super::worker::Device;
use super::Tasks;

pub(super) enum Change {
    Effect(Effect),
    Brightness(u16),
    Colors {
        slot: u8,
        brightness: u16,
        frame: Box<RgbFrame>,
    },
}

impl Change {
    fn name(&self) -> &'static str {
        match self {
            Self::Effect(_) => "effect",
            Self::Brightness(_) => "brightness",
            Self::Colors { .. } => "custom palette",
        }
    }

    fn send(&self, device: &mut Device, region: Region) -> io::Result<Effect> {
        if device.reactive_active() {
            return Err(io::Error::new(
                io::ErrorKind::ResourceBusy,
                "Turn off reactive lighting before changing the saved effect.",
            ));
        }
        if let Self::Colors {
            slot,
            brightness,
            frame,
        } = self
        {
            return device.apply_palette(*slot, *brightness, frame);
        }
        let keyboard = device.keyboard()?;
        keyboard.recover_reads()?;
        match self {
            Self::Effect(effect) => {
                let effect = if region == Region::Keyboard {
                    with_brightness(*effect, keyboard.read_lighting(region)?.brightness)
                } else {
                    *effect
                };
                lighting_trace::event(format_args!("write settings {region:?}: {effect:?}"));
                keyboard.set_lighting(region, effect, timestamp()?)?;
            }
            Self::Brightness(brightness) => {
                let current = keyboard.read_lighting(region)?;
                lighting_trace::event(format_args!(
                    "brightness {region:?}: {current:?} -> {brightness}"
                ));
                if current.brightness == *brightness {
                    return Ok(current);
                }
                keyboard.set_lighting(
                    region,
                    with_brightness(current, *brightness),
                    timestamp()?,
                )?;
            }
            Self::Colors { .. } => unreachable!(),
        }
        let effect = keyboard.read_lighting(region)?;
        device.persist_effect(region, effect)?;
        Ok(effect)
    }

    fn same_target(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Effect(_), Self::Effect(_)) | (Self::Brightness(_), Self::Brightness(_)) => true,
            (Self::Colors { slot: left, .. }, Self::Colors { slot: right, .. }) => left == right,
            _ => false,
        }
    }

    fn custom(&self) -> bool {
        matches!(self, Self::Colors { .. })
    }
}

fn with_brightness(effect: Effect, brightness: u16) -> Effect {
    Effect {
        brightness,
        ..effect
    }
}

fn timestamp() -> io::Result<u16> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|time| time.as_millis() as u16)
        .map_err(io::Error::other)
}

type Done = Box<dyn FnOnce(io::Result<Effect>)>;
type Listener = Box<dyn Fn(Region, bool, Option<Effect>)>;
struct Edit {
    region: Region,
    change: std::sync::Arc<Change>,
    done: Done,
}

pub(super) struct Changes {
    tasks: Rc<Tasks>,
    revision: u64,
    queued: RefCell<VecDeque<Edit>>,
    active: Cell<Option<Region>>,
    in_flight_done: RefCell<Option<Done>>,
    listeners: RefCell<Vec<Listener>>,
    scheduled: Cell<bool>,
    latest: Cell<Instant>,
}

impl Changes {
    pub fn new(tasks: &Rc<Tasks>) -> Rc<Self> {
        Rc::new(Self {
            tasks: tasks.clone(),
            revision: tasks.revision.get(),
            queued: RefCell::new(VecDeque::new()),
            active: Cell::new(None),
            in_flight_done: RefCell::new(None),
            listeners: RefCell::new(Vec::new()),
            scheduled: Cell::new(false),
            latest: Cell::new(Instant::now()),
        })
    }

    pub fn pending(&self, region: Region) -> bool {
        self.active.get() == Some(region)
            || self
                .queued
                .borrow()
                .iter()
                .any(|edit| edit.region == region)
    }

    pub fn queued_colors(&self, slot: u8) -> bool {
        self.queued.borrow().iter().any(
            |edit| matches!(*edit.change, Change::Colors { slot: target, .. } if target == slot),
        )
    }

    pub fn listen(&self, update: impl Fn(Region, bool, Option<Effect>) + 'static) {
        self.listeners.borrow_mut().push(Box::new(update));
    }

    pub fn queue(
        self: &Rc<Self>,
        region: Region,
        change: Change,
        done: impl FnOnce(io::Result<Effect>) + 'static,
    ) {
        let mut queued = self.queued.borrow_mut();
        queued.retain(|edit| edit.region != region || !edit.change.same_target(&change));
        lighting_trace::event(format_args!("queue {region:?} {}", change.name()));
        queued.push_back(Edit {
            region,
            change: std::sync::Arc::new(change),
            done: Box::new(done),
        });
        self.latest.set(Instant::now());
        if self.scheduled.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(30), move || {
            let Some(changes) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if changes.revision != changes.tasks.revision.get()
                || changes.queued.borrow().is_empty()
            {
                changes.scheduled.set(false);
                return glib::ControlFlow::Break;
            }
            if changes.latest.get().elapsed() >= Duration::from_millis(120) {
                changes.send_next();
            }
            glib::ControlFlow::Continue
        });
    }

    fn send_next(self: &Rc<Self>) {
        if self.active.get().is_some() {
            return;
        }
        let mut queued = self.queued.borrow_mut();
        let Some(edit) = queued.pop_front() else {
            return;
        };
        let region = edit.region;
        let change = edit.change.clone();
        let custom = change.custom();
        let weak = Rc::downgrade(self);
        if self.tasks.background_device(
            move |device| {
                let result = change.send(device, region);
                lighting_trace::event(format_args!(
                    "write result {region:?} {}: {result:?}",
                    change.name()
                ));
                result
            },
            move |result| {
                if let Some(changes) = weak.upgrade() {
                    changes.active.set(None);
                    let edit = changes.in_flight_done.borrow_mut().take();
                    let actual = result.as_ref().ok().copied();
                    if let Some(done) = edit {
                        done(result);
                    }
                    for update in changes.listeners.borrow().iter() {
                        update(region, custom, actual);
                    }
                }
            },
        ) {
            self.active.set(Some(region));
            *self.in_flight_done.borrow_mut() = Some(edit.done);
        } else {
            queued.push_front(edit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_only_coalesces_edits_for_the_same_setting_or_slot() {
        let colors = |slot| Change::Colors {
            slot,
            brightness: 5,
            frame: Box::default(),
        };
        assert!(colors(0).same_target(&colors(0)));
        assert!(!colors(0).same_target(&colors(1)));
        assert!(!colors(0).same_target(&Change::Brightness(3)));
        let effect = Effect {
            mode: 10,
            brightness: 5,
            speed: 1,
            foreground: [0; 3],
            background: [0; 3],
            direction: 0,
            effect_type: 0,
            open: 0,
        };
        assert!(!colors(0).same_target(&Change::Effect(effect)));
    }

    #[test]
    fn brightness_only_changes_the_level_byte_even_in_custom_mode() {
        let saved = Effect {
            mode: 10,
            brightness: 4,
            speed: 3,
            foreground: [1, 2, 3],
            background: [4, 5, 6],
            direction: 6,
            effect_type: 0,
            open: 0,
        };
        let before = Region::Keyboard.effect(saved, 0).unwrap();
        let after = Region::Keyboard
            .effect(with_brightness(saved, 2), 0)
            .unwrap();
        assert_eq!(after[7], 1);
        assert_eq!(
            before
                .iter()
                .zip(after.iter())
                .enumerate()
                .filter_map(|(i, (a, b))| (a != b).then_some(i))
                .collect::<Vec<_>>(),
            [7]
        );
    }
}
