use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::io;
use std::rc::Rc;
use std::time::Duration;

use dragon_driver::keyboard::{
    hall::{Control, Readback},
    lighting::{Effect, Region, RgbFrame, SupportedModes},
    models::Position,
    performance::{LinkStatus, Sleep},
    session::Keyboard,
};
use gtk::glib;

use super::keys::KeyGrid;
use super::lighting_trace;
use super::worker::{Connected, Device, Tasks};

#[derive(Clone, Default, Eq, PartialEq)]
pub(super) struct Snapshot {
    pub link: Option<LinkStatus>,
    pub limit: Option<u8>,
    pub lighting: Option<(SupportedModes, Effect)>,
    pub palettes: Option<[Option<RgbFrame>; 2]>,
    pub side: Option<(SupportedModes, Effect)>,
    pub sleep: Option<Sleep>,
    pub anti_shake: Option<u8>,
    pub return_rate: Option<u8>,
}

fn probe<T>(
    keyboard: &mut Keyboard,
    read: impl FnOnce(&mut Keyboard) -> io::Result<T>,
) -> Option<T> {
    keyboard.recover_reads().and_then(|()| read(keyboard)).ok()
}

fn trace_effect(keyboard: &mut Keyboard, region: Region) -> io::Result<Effect> {
    let result = keyboard.read_lighting(region);
    lighting_trace::event(format_args!("poll {region:?}: {result:?}"));
    result
}

#[derive(Clone, Copy)]
enum WatchedPage {
    Lighting,
    Device,
    Other,
}

impl Snapshot {
    fn read(keyboard: &mut Keyboard) -> Self {
        let read_lighting = |region| {
            move |keyboard: &mut Keyboard| {
                let modes = keyboard.read_lighting_modes(region).inspect_err(|error| {
                    lighting_trace::event(format_args!("catalog {region:?} failed: {error}"));
                })?;
                lighting_trace::event(format_args!(
                    "catalog {region:?}: {:?} levels {:?}",
                    modes.modes, modes.settings
                ));
                Ok((modes, trace_effect(keyboard, region)?))
            }
        };
        let layout = keyboard.layout();
        Self {
            link: probe(keyboard, Keyboard::read_link_status),
            limit: layout
                .magnetic()
                .then(|| probe(keyboard, Keyboard::actuation_limit))
                .flatten(),
            lighting: probe(keyboard, read_lighting(Region::Keyboard)),
            palettes: None,
            side: layout
                .side_lighting
                .then(|| probe(keyboard, read_lighting(Region::Side)))
                .flatten(),
            sleep: probe(keyboard, Keyboard::read_sleep),
            anti_shake: layout
                .magnetic()
                .then(|| {
                    probe(keyboard, |keyboard| {
                        keyboard.read_control(Control::AntiShake)
                    })
                })
                .flatten(),
            return_rate: layout
                .magnetic()
                .then(|| {
                    probe(keyboard, |keyboard| {
                        keyboard.read_control(Control::ReturnRate)
                    })
                })
                .flatten(),
        }
    }
    fn update_visible(mut self, page: WatchedPage, device: &mut Device) -> io::Result<Self> {
        let keyboard = device.keyboard()?;
        let mut observed = [None; 2];
        if let Some((_, effect)) = &mut self.lighting
            && let Some(current) = probe(keyboard, |keyboard| {
                trace_effect(keyboard, Region::Keyboard)
            })
        {
            *effect = current;
            observed[Region::Keyboard.index()] = Some(current);
        }
        match page {
            WatchedPage::Lighting => {
                if let Some((_, effect)) = &mut self.side
                    && let Some(current) =
                        probe(keyboard, |keyboard| trace_effect(keyboard, Region::Side))
                {
                    *effect = current;
                    observed[Region::Side.index()] = Some(current);
                }
            }
            WatchedPage::Device => {
                if self.link.is_some() {
                    self.link = probe(keyboard, Keyboard::read_link_status);
                }
                if self.sleep.is_some() {
                    self.sleep = probe(keyboard, Keyboard::read_sleep);
                }
                if self.anti_shake.is_some() {
                    self.anti_shake = probe(keyboard, |keyboard| {
                        keyboard.read_control(Control::AntiShake)
                    });
                }
                if self.return_rate.is_some() {
                    self.return_rate = probe(keyboard, |keyboard| {
                        keyboard.read_control(Control::ReturnRate)
                    });
                }
            }
            WatchedPage::Other => {}
        }
        for region in [Region::Keyboard, Region::Side] {
            if let Some(effect) = observed[region.index()] {
                device.observe_effect(region, effect);
            }
        }
        self.palettes = Some(device.saved_palettes());
        Ok(self)
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum KeyState {
    Pending,
    Unavailable,
    Confirmed(Readback),
}

type SnapshotListener = Box<dyn Fn(&Snapshot)>;
type KeyListener = Box<dyn Fn(KeyState)>;

pub(super) struct Readbacks {
    tasks: Rc<Tasks>,
    revision: u64,
    key_epoch: Cell<u64>,
    positions: Vec<Position>,
    pending: RefCell<VecDeque<Position>>,
    cache: RefCell<HashMap<Position, KeyState>>,
    selected: Cell<Option<Position>>,
    key: RefCell<Option<KeyListener>>,
    snapshot: RefCell<Vec<SnapshotListener>>,
    latest: RefCell<Option<Snapshot>>,
    refresh_pending: Cell<bool>,
    valid_limit: Cell<bool>,
    sweep: Cell<bool>,
    scheduled: Cell<bool>,
}

impl Readbacks {
    pub fn new(device: &Connected, grid: &KeyGrid, tasks: &Rc<Tasks>) -> Rc<Self> {
        let positions: Vec<_> = if device.layout.magnetic() {
            grid.buttons.iter().map(|(key, _)| *key).collect()
        } else {
            Vec::new()
        };
        Rc::new(Self {
            tasks: tasks.clone(),
            revision: tasks.revision.get(),
            key_epoch: Cell::new(0),
            pending: RefCell::new(positions.iter().copied().collect()),
            positions,
            cache: RefCell::new(HashMap::new()),
            selected: Cell::new(None),
            key: RefCell::new(None),
            snapshot: RefCell::new(Vec::new()),
            latest: RefCell::new(None),
            refresh_pending: Cell::new(false),
            valid_limit: Cell::new(false),
            sweep: Cell::new(true),
            scheduled: Cell::new(false),
        })
    }

    pub fn on_snapshot(&self, update: impl Fn(&Snapshot) + 'static) {
        self.snapshot.borrow_mut().push(Box::new(update));
    }

    pub fn on_key(&self, update: impl Fn(KeyState) + 'static) {
        *self.key.borrow_mut() = Some(Box::new(update));
    }

    fn display(&self, status: KeyState) {
        if let Some(update) = self.key.borrow().as_ref() {
            update(status);
        }
    }

    pub fn select(self: &Rc<Self>, key: Option<Position>) {
        if self.selected.replace(key) == key {
            return;
        }
        if let Some(key) = key {
            let state = self
                .cache
                .borrow()
                .get(&key)
                .copied()
                .unwrap_or(KeyState::Pending);
            self.display(state);
            self.schedule();
        }
    }

    pub fn refresh(self: &Rc<Self>) {
        self.refresh_pending.set(true);
        self.latest.borrow_mut().take();
        self.cache.borrow_mut().clear();
        self.key_epoch.set(self.key_epoch.get().wrapping_add(1));
        self.sweep.set(true);
        self.schedule();
    }

    pub fn invalidate(self: &Rc<Self>, keys: &[Position]) {
        for key in keys {
            self.cache.borrow_mut().remove(key);
        }
        self.schedule();
    }

    pub fn invalidate_all(self: &Rc<Self>) {
        *self.pending.borrow_mut() = self.positions.iter().copied().collect();
        self.refresh();
    }

    pub fn confirm(&self, key: Position, value: Readback) {
        self.cache
            .borrow_mut()
            .insert(key, KeyState::Confirmed(value));
        if self.selected.get() == Some(key) {
            self.display(KeyState::Confirmed(value));
        }
    }

    pub(super) fn publish(&self, snapshot: Snapshot) {
        let mut latest = self.latest.borrow_mut();
        if latest.as_ref() == Some(&snapshot) {
            return;
        }
        for update in self.snapshot.borrow().iter() {
            update(&snapshot);
        }
        *latest = Some(snapshot);
    }

    pub fn poll_visible(self: &Rc<Self>, page: &str) {
        if self.scheduled.get() || self.refresh_pending.get() {
            return;
        }
        let epoch = self.key_epoch.get();
        let weak = Rc::downgrade(self);
        let key = self.selected.get().filter(|key| {
            page == "actuation"
                && matches!(self.cache.borrow().get(key), Some(KeyState::Confirmed(_)))
        });
        let page = match page {
            "lighting" => WatchedPage::Lighting,
            "device" => WatchedPage::Device,
            _ => WatchedPage::Other,
        };
        let Some(latest) = self.latest.borrow().clone() else {
            return;
        };
        self.tasks.background_device(
            move |device| {
                let snapshot = latest.update_visible(page, device)?;
                let value = key.and_then(|key| {
                    probe(device.keyboard().ok()?, |keyboard| keyboard.read_key(key))
                });
                Ok((snapshot, value))
            },
            move |result| {
                if let Some(state) = weak.upgrade()
                    && state.key_epoch.get() == epoch
                    && let Ok((snapshot, value)) = result
                {
                    state.publish(snapshot);
                    if let (Some(key), Some(value)) = (key, value) {
                        state.confirm(key, value);
                    }
                }
            },
        );
    }

    fn schedule(self: &Rc<Self>) {
        if self.scheduled.replace(true) {
            return;
        }
        let weak = Rc::downgrade(self);
        glib::timeout_add_local(Duration::from_millis(20), move || {
            let Some(state) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if state.revision == state.tasks.revision.get() && state.advance() {
                return glib::ControlFlow::Continue;
            }
            state.scheduled.set(false);
            glib::ControlFlow::Break
        });
    }

    fn advance(self: &Rc<Self>) -> bool {
        if self.refresh_pending.get() {
            let weak = Rc::downgrade(self);
            if self.tasks.background_device(
                |device| {
                    let mut snapshot = Snapshot::read(device.keyboard()?);
                    if let Some((_, effect)) = &snapshot.lighting {
                        device.observe_effect(Region::Keyboard, *effect);
                    }
                    if let Some((_, effect)) = &snapshot.side {
                        device.observe_effect(Region::Side, *effect);
                    }
                    snapshot.palettes = Some(device.saved_palettes());
                    Ok(snapshot)
                },
                move |result| {
                    if let Some(state) = weak.upgrade() {
                        let snapshot = result.unwrap_or_default();
                        state.valid_limit.set(snapshot.limit.is_some());
                        state.publish(snapshot);
                        if !state.valid_limit.get() && state.selected.get().is_some() {
                            state.display(KeyState::Unavailable);
                        }
                        state.schedule();
                    }
                },
            ) {
                self.refresh_pending.set(false);
                return false;
            }
            return true;
        }
        if !self.valid_limit.get() {
            return false;
        }

        let mut pending = self.pending.borrow_mut();
        let cache = self.cache.borrow();
        while pending.front().is_some_and(|key| cache.contains_key(key)) {
            pending.pop_front();
        }
        let key = self
            .selected
            .get()
            .filter(|key| !cache.contains_key(key))
            .or_else(|| self.sweep.get().then(|| pending.front().copied()).flatten());
        let Some(key) = key else { return false };
        drop(cache);
        let epoch = self.key_epoch.get();
        let weak = Rc::downgrade(self);
        if self.tasks.background_keyboard(
            move |keyboard| {
                keyboard.recover_reads()?;
                keyboard.read_key(key)
            },
            move |result| {
                if let Some(state) = weak.upgrade() {
                    if state.key_epoch.get() != epoch {
                        if !state.pending.borrow().contains(&key) {
                            state.pending.borrow_mut().push_front(key);
                        }
                        state.schedule();
                        return;
                    }
                    match result {
                        Ok(value) => state.confirm(key, value),
                        Err(_) => {
                            state.sweep.set(false);
                            state.cache.borrow_mut().insert(key, KeyState::Unavailable);
                            if state.selected.get() == Some(key) {
                                state.display(KeyState::Unavailable);
                            }
                        }
                    }
                    state.schedule();
                }
            },
        ) {
            if pending.front() == Some(&key) {
                pending.pop_front();
            }
            return false;
        }
        true
    }
}
