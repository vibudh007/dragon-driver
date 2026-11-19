use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::Ordering;

use dragon_driver::keyboard::{
    lighting::{Effect, Region, RgbFrame},
    models::Position,
};
use dragon_driver::witmod_sdk::keymap::{KEY_COLUMNS, KEY_ROWS};
use gtk::{Orientation, glib, prelude::*};

use super::super::color_wheel::{ColorWheel, rainbow_button};
use super::super::keys::KeyGrid;
use super::super::lighting_trace;
use super::super::readbacks::{Readbacks, Snapshot};
use super::super::worker::{Connected, Tasks};
use super::super::{field, label, page, panel, primary};
use super::changes::{Change, Changes};
use super::reactive::reactive_panels;

pub(in super::super) fn lighting(
    device: &Connected,
    grid: &KeyGrid,
    tasks: &Rc<Tasks>,
    readbacks: &Readbacks,
) -> gtk::Box {
    let page = page("Lighting", "display-brightness-symbolic");
    let changes = Changes::new(tasks);
    let swatches = KeyboardSwatches::new();
    let manual = gtk::Box::new(Orientation::Vertical, 12);
    let (color, editor) = custom_colors(
        grid,
        &changes,
        readbacks,
        device.palettes,
        device.palette_slot,
        &swatches,
    );
    manual.append(&color);
    manual.append(&effects(
        Region::Keyboard,
        &changes,
        readbacks,
        Some(&swatches),
    ));
    if device.layout.side_lighting {
        manual.append(&effects(Region::Side, &changes, readbacks, None));
    }
    if device.transport == Some(1) {
        manual.append(&wireless_panel(tasks));
    }
    page.append(&manual);
    let (manual, tools, editor) = (
        manual.downgrade(),
        swatches.stack.downgrade(),
        Rc::downgrade(&editor),
    );
    if let Some(reactive) = reactive_panels(device, tasks, readbacks, move |active, frame| {
        if let (Some(manual), Some(tools), Some(editor)) =
            (manual.upgrade(), tools.upgrade(), editor.upgrade())
        {
            manual.set_sensitive(!active);
            tools.set_sensitive(!active);
            if editor.reactive_active.replace(active) != active {
                editor.original_colors.borrow_mut().clear();
                editor.selection_changes.borrow_mut().clear();
            }
            editor.wallpaper_frame.set(frame);
            editor.draw();
        }
    }) {
        page.append(&reactive);
    }
    page
}

struct KeyboardSwatches {
    stack: gtk::Stack,
    brush: gtk::Box,
    effects: gtk::Box,
}

impl KeyboardSwatches {
    fn new() -> Self {
        let stack = gtk::Stack::new();
        let brush = gtk::Box::new(Orientation::Horizontal, 0);
        let effects = gtk::Box::new(Orientation::Horizontal, 6);
        brush.set_halign(gtk::Align::End);
        effects.set_halign(gtk::Align::End);
        stack.add_named(&brush, Some("brush"));
        stack.add_named(&effects, Some("effects"));
        Self {
            stack,
            brush,
            effects,
        }
    }
}

struct EffectEditor {
    changes: Rc<Changes>,
    region: Region,
    loading: Cell<bool>,
    saved: Cell<Option<Effect>>,
    modes: RefCell<Vec<u8>>,
    settings: gtk::Box,
    status: gtk::Label,
    mode: gtk::ComboBoxText,
    brightness: gtk::SpinButton,
    speed: gtk::SpinButton,
    foreground: ColorWheel,
    background: ColorWheel,
    direction: Cell<u8>,
    direction_row: gtk::Box,
    direction_buttons: [gtk::ToggleButton; 8],
    rainbow: gtk::ToggleButton,
    speed_row: gtk::Box,
    colors: gtk::Widget,
    swatches: Option<glib::WeakRef<gtk::Stack>>,
}

fn effects(
    region: Region,
    changes: &Rc<Changes>,
    readbacks: &Readbacks,
    swatches: Option<&KeyboardSwatches>,
) -> gtk::Box {
    let panel = panel(match region {
        Region::Keyboard => "Lighting effects",
        Region::Side => "Side lighting",
    });
    let settings = gtk::Box::new(Orientation::Vertical, 8);
    settings.set_sensitive(false);
    let mode = gtk::ComboBoxText::new();
    let brightness = gtk::SpinButton::with_range(1.0, 256.0, 1.0);
    let speed = gtk::SpinButton::with_range(1.0, 256.0, 1.0);
    let foreground = ColorWheel::new("Choose effect color");
    let background = ColorWheel::new("Choose background color");
    let rainbow = rainbow_button();
    settings.append(&field("Effect", &mode));
    if region == Region::Side {
        settings.append(&field("Brightness", &brightness));
    }
    let speed_row = field("Speed", &speed);
    settings.append(&speed_row);
    let directions = gtk::Box::new(Orientation::Horizontal, 0);
    directions.add_css_class("linked");
    let direction_buttons = DIRECTIONS.map(|_| {
        let button = gtk::ToggleButton::new();
        directions.append(&button);
        button
    });
    for button in &direction_buttons[1..] {
        button.set_group(Some(&direction_buttons[0]));
    }
    let direction_row = field("Direction", &directions);
    direction_row.set_visible(false);
    settings.append(&direction_row);
    let color_controls = swatches.map_or_else(
        || gtk::Box::new(Orientation::Horizontal, 6),
        |swatches| swatches.effects.clone(),
    );
    color_controls.append(&rainbow);
    color_controls.append(background.widget());
    color_controls.append(foreground.widget());
    color_controls.set_sensitive(false);
    let colors = if swatches.is_some() {
        color_controls.upcast()
    } else {
        let row = field("Colors", &color_controls);
        row.set_visible(false);
        settings.append(&row);
        row.upcast()
    };

    panel.append(&settings);
    let status = label("Checking the saved effect…", "muted");
    panel.append(&status);

    let editor = Rc::new(EffectEditor {
        changes: changes.clone(),
        region,
        loading: Cell::new(false),
        saved: Cell::new(None),
        modes: RefCell::new(Vec::new()),
        settings,
        status,
        mode,
        brightness,
        speed,
        foreground,
        background,
        direction: Cell::new(0),
        direction_row,
        direction_buttons,
        rainbow,
        speed_row,
        colors,
        swatches: swatches.map(|swatches| swatches.stack.downgrade()),
    });
    let owned = editor.clone();
    panel.connect_destroy(move |_| {
        let _ = &owned;
    });
    let weak = Rc::downgrade(&editor);
    readbacks.on_snapshot(move |snapshot| {
        if let Some(editor) = weak.upgrade() {
            editor.snapshot(snapshot);
        }
    });
    let weak = Rc::downgrade(&editor);
    changes.listen(move |region, _, actual| {
        if let Some(editor) = weak.upgrade()
            && editor.region == region
            && !editor.changes.pending(region)
            && let Some(effect) = actual
        {
            editor.show(effect);
        }
    });

    let weak = Rc::downgrade(&editor);
    let commit = move |choose_color| {
        if let Some(editor) = weak.upgrade() {
            editor.commit(choose_color);
        }
    };
    let send = commit.clone();
    editor.mode.connect_changed(move |_| send(false));
    for spin in [&editor.brightness, &editor.speed] {
        let send = commit.clone();
        spin.connect_value_changed(move |_| send(false));
    }
    let send = commit.clone();
    editor.foreground.connect_color_set(move |_| send(true));
    let send = commit.clone();
    editor.background.connect_color_set(move |_| send(true));
    let send = commit.clone();
    editor.rainbow.connect_toggled(move |_| send(false));
    for (index, button) in editor.direction_buttons.iter().enumerate() {
        let weak = Rc::downgrade(&editor);
        button.connect_toggled(move |button| {
            if let Some(editor) = weak.upgrade()
                && !editor.loading.get()
                && button.is_active()
            {
                editor.direction.set(index as u8);
                editor.commit(false);
            }
        });
    }
    panel
}

impl EffectEditor {
    fn snapshot(&self, snapshot: &Snapshot) {
        if self.changes.pending(self.region) {
            return;
        }
        let reading = match self.region {
            Region::Keyboard => &snapshot.lighting,
            Region::Side => &snapshot.side,
        };
        let Some((modes, effect)) = reading else {
            self.settings.set_sensitive(false);
            self.colors.set_sensitive(false);
            self.status.set_label("Couldn’t read the saved effect.");
            return;
        };
        self.loading.set(true);
        if *self.modes.borrow() != modes.modes {
            self.mode.remove_all();
            for value in modes.modes.iter().copied().filter(|&value| value != 100) {
                self.mode
                    .append(Some(&value.to_string()), &mode_name(self.region, value));
            }
            self.modes.replace(modes.modes.clone());
        }
        let [brightness, speed] = modes.settings.map(|value| u16::from(value) + 1);
        self.brightness
            .set_range(1.0, f64::from(brightness.max(effect.brightness)));
        self.speed
            .set_range(1.0, f64::from(speed.max(effect.speed)));
        self.settings.set_sensitive(true);
        self.colors.set_sensitive(true);
        self.show(*effect);
        self.status.set_label("");
    }

    fn show(&self, effect: Effect) {
        self.saved.set(Some(effect));
        self.loading.set(true);
        self.mode.set_active_id(Some(&effect.mode.to_string()));
        self.brightness.set_value(effect.brightness.into());
        self.speed.set_value(effect.speed.into());
        self.foreground.set_rgb(effect.foreground);
        self.background.set_rgb(effect.background);
        self.direction.set(effect.direction);
        self.rainbow.set_active(effect.effect_type != 0);
        self.update_controls();
        self.loading.set(false);
    }

    fn update_controls(&self) {
        let mode = self.mode.active_id().and_then(|id| id.parse::<u8>().ok());
        let custom = self.region == Region::Keyboard && mode == Some(10);
        self.brightness.set_sensitive(!custom);
        self.speed_row.set_visible(!custom && mode != Some(0));
        let foreground = mode.is_some_and(|mode| effect_has_color(self.region, mode));
        let rainbow = mode.is_some_and(|mode| effect_has_rainbow(self.region, mode));
        let background = mode.is_some_and(|mode| effect_has_background(self.region, mode));
        self.foreground.widget().set_visible(foreground);
        self.rainbow.set_visible(rainbow);
        self.background.widget().set_visible(background);
        let has_colors = foreground || rainbow || background;
        if let Some(stack) = self.swatches.as_ref().and_then(glib::WeakRef::upgrade) {
            stack.set_visible_child_name(if has_colors { "effects" } else { "brush" });
        } else {
            self.colors.set_visible(has_colors);
        }
        self.update_direction(mode);
    }

    fn update_direction(&self, mode: Option<u8>) {
        let options = mode.map_or(&[][..], |mode| direction_options(self.region, mode));
        self.direction_row.set_visible(!options.is_empty());
        let raw = self.direction.get();
        let mut displayed = mode.and_then(|mode| display_direction(self.region, mode, raw));
        if displayed.is_none()
            && self.saved.get().map(|effect| effect.mode) != mode
            && let Some(&first) = options.first()
        {
            self.direction.set(first);
            displayed = Some(first);
        }
        let loading = self.loading.replace(true);
        for (index, button) in self.direction_buttons.iter().enumerate() {
            let index = index as u8;
            button.set_visible(options.contains(&index));
            let (glyph, name) = direction_label(mode, index);
            button.set_label(glyph);
            button.set_tooltip_text(Some(name));
            button.update_property(&[gtk::accessible::Property::Label(name)]);
            button.set_active(displayed == Some(index));
        }
        self.loading.set(loading);
    }

    fn commit(self: &Rc<Self>, choose_color: bool) {
        if self.loading.get() {
            return;
        }
        self.update_controls();
        let (Some(previous), Some(mode)) = (
            self.saved.get(),
            self.mode.active_id().and_then(|id| id.parse::<u8>().ok()),
        ) else {
            return;
        };
        if self.region == Region::Keyboard && mode == 10 {
            if previous.mode != 10 {
                self.status
                    .set_label("Choose a color above to turn on per-key lighting.");
            }
            return;
        }
        if choose_color && effect_has_rainbow(self.region, mode) {
            self.loading.set(true);
            self.rainbow.set_active(false);
            self.loading.set(false);
        }
        let effect = Effect {
            mode,
            brightness: self.brightness.value_as_int() as u16,
            speed: self.speed.value_as_int() as u16,
            foreground: self.foreground.rgb(),
            background: self.background.rgb(),
            direction: self.direction.get(),
            effect_type: if effect_has_rainbow(self.region, mode) {
                u8::from(self.rainbow.is_active())
            } else if mode == 0 {
                0
            } else {
                previous.effect_type
            },
            open: previous.open,
        };
        if effect == previous && !self.changes.pending(self.region) {
            return;
        }
        self.status.set_label("Changing the effect…");
        let weak = Rc::downgrade(self);
        self.changes.queue(self.region, Change::Effect(effect), move |result| {
            if let Some(editor) = weak.upgrade()
                && !editor.changes.pending(editor.region)
            {
                editor.status.set_label(match result {
                    Ok(actual) if effect_matches(editor.region, previous, effect, actual) => "Effect updated.",
                    Ok(_) => "The keyboard saved different settings.",
                    Err(_) => "Couldn’t confirm the effect. Try changing it again.",
                });
                if result.is_err() {
                    editor.saved.set(None);
                    editor.status.set_label("Couldn’t confirm the effect. Reopen Lighting after the keyboard reconnects.");
                }
            }
        });
    }
}

const DIRECTIONS: [(&str, &str); 8] = [
    ("→", "Left to right"),
    ("←", "Right to left"),
    ("↑", "Bottom to top"),
    ("↓", "Top to bottom"),
    ("Outward", "Outward"),
    ("Inward", "Inward"),
    ("↻", "Clockwise"),
    ("↺", "Counterclockwise"),
];
const ROTATION: &[u8] = &[6, 7];

fn direction_label(mode: Option<u8>, index: u8) -> (&'static str, &'static str) {
    let index = if mode == Some(15) && index >= 6 {
        index - 6
    } else {
        index
    };
    DIRECTIONS[usize::from(index)]
}

fn direction_options(region: Region, mode: u8) -> &'static [u8] {
    match (region, mode) {
        (Region::Keyboard, 2) => &[0, 1, 2, 3, 4, 5],
        (Region::Keyboard, 4 | 5 | 15) => ROTATION,
        (Region::Keyboard, 7) => &[4, 5],
        (Region::Keyboard, 12) => &[2, 3],
        _ => &[],
    }
}

fn display_direction(region: Region, mode: u8, raw: u8) -> Option<u8> {
    let options = direction_options(region, mode);
    if options.contains(&raw) {
        Some(raw)
    } else if options == ROTATION && raw <= 1 {
        Some(raw + 6)
    } else {
        None
    }
}

fn effect_has_rainbow(region: Region, mode: u8) -> bool {
    match region {
        Region::Keyboard => matches!(mode, 1..=9 | 11 | 12 | 14..=16),
        Region::Side => matches!(mode, 1..=4),
    }
}

fn effect_has_color(region: Region, mode: u8) -> bool {
    mode == 0 || effect_has_rainbow(region, mode)
}

fn effect_has_background(region: Region, mode: u8) -> bool {
    match region {
        Region::Keyboard => matches!(mode, 1 | 2 | 4..=9 | 11 | 14),
        Region::Side => matches!(mode, 1..=3),
    }
}

fn effect_matches(region: Region, previous: Effect, expected: Effect, actual: Effect) -> bool {
    let fixed = expected.effect_type == 0;
    expected.mode == actual.mode
        && display_direction(region, expected.mode, expected.direction).is_none_or(|direction| {
            display_direction(region, actual.mode, actual.direction) == Some(direction)
        })
        && (region == Region::Keyboard || expected.brightness == actual.brightness)
        && (!effect_has_rainbow(region, expected.mode) || fixed == (actual.effect_type == 0))
        && (!effect_has_color(region, expected.mode)
            || !fixed
            || expected.foreground == actual.foreground)
        && (!effect_has_background(region, expected.mode)
            || !fixed
            || expected.background == previous.background
            || expected.background == actual.background)
}

fn mode_name(region: Region, mode: u8) -> String {
    let names: &[&str] = match region {
        Region::Keyboard => &[
            "Static",
            "Breathing",
            "Wave",
            "Neon",
            "Radar",
            "Snake",
            "Reactive",
            "Aurora",
            "Ripple",
            "Twinkle",
            "Custom per-key",
            "Cross",
            "Speed response",
            "DPI",
            "Auto ripple",
            "Striation",
            "Fireworks",
        ],
        Region::Side => &["Static", "Breathing", "Wave", "Collision", "Neon", "Off"],
    };
    if mode == 100 {
        return "Audio reactive".into();
    }
    names
        .get(usize::from(mode))
        .map_or_else(|| format!("Effect {mode}"), |name| (*name).into())
}

type SelectionChange = (Position, Option<[u8; 3]>);

struct ColorEditor {
    changes: Rc<Changes>,
    visible_keys: Rc<Vec<(Position, gtk::ToggleButton)>>,
    provider: gtk::CssProvider,
    color: ColorWheel,
    brightness: gtk::Scale,
    slot: gtk::ComboBoxText,
    status: gtk::Label,
    fill: gtk::Button,
    palettes: Cell<[Option<RgbFrame>; 2]>,
    painted: Cell<Option<RgbFrame>>,
    reactive_active: Cell<bool>,
    wallpaper_frame: Cell<Option<RgbFrame>>,
    current: Cell<Option<Effect>>,
    loading: Cell<bool>,
    selecting: Cell<bool>,
    area: glib::WeakRef<gtk::Box>,
    selection_changes: RefCell<Vec<SelectionChange>>,
    original_colors: RefCell<SelectionColors>,
}

fn custom_colors(
    grid: &KeyGrid,
    changes: &Rc<Changes>,
    readbacks: &Readbacks,
    palettes: [Option<RgbFrame>; 2],
    palette_slot: u8,
    swatches: &KeyboardSwatches,
) -> (gtk::Box, Rc<ColorEditor>) {
    let panel = panel("Key colors");
    let controls = gtk::Box::new(Orientation::Horizontal, 12);
    grid.lighting_tools.append(&controls);
    let color = ColorWheel::new("Choose key color");
    swatches.brush.append(color.widget());
    controls.append(&swatches.stack);
    let brush_in_panel = gtk::Box::new(Orientation::Horizontal, 0);
    let brush_row = field("Paint color", &brush_in_panel);
    brush_row.set_visible(false);
    panel.append(&brush_row);
    let (brush, toolbar, inline, row) = (
        color.widget().downgrade(),
        swatches.brush.downgrade(),
        brush_in_panel.downgrade(),
        brush_row.downgrade(),
    );
    swatches
        .stack
        .connect_visible_child_name_notify(move |stack| {
            if let (Some(brush), Some(toolbar), Some(inline), Some(row)) = (
                brush.upgrade(),
                toolbar.upgrade(),
                inline.upgrade(),
                row.upgrade(),
            ) {
                let use_inline = stack.visible_child_name().as_deref() == Some("effects");
                let (from, to) = if use_inline {
                    (&toolbar, &inline)
                } else {
                    (&inline, &toolbar)
                };
                if brush.parent().as_ref() == Some(from.upcast_ref()) {
                    from.remove(&brush);
                    to.append(&brush);
                }
                row.set_visible(use_inline);
            }
        });
    let brightness = gtk::Scale::with_range(Orientation::Horizontal, 1.0, 5.0, 1.0);
    brightness.set_digits(0);
    brightness.set_draw_value(false);
    brightness.set_width_request(128);
    brightness.set_tooltip_text(Some("Keyboard brightness"));
    brightness.update_property(&[gtk::accessible::Property::Label("Keyboard brightness")]);
    controls.append(&brightness);
    brightness.set_sensitive(false);

    let slot = gtk::ComboBoxText::new();
    slot.append_text("Custom 1");
    slot.append_text("Custom 2");
    slot.set_active(Some(u32::from(palette_slot)));
    slot.set_tooltip_text(Some("The next color change is saved to this slot"));
    panel.append(&field("Save colors to", &slot));
    let status = label("Checking saved colors…", "muted");
    status.set_wrap(true);
    let fill = gtk::Button::with_label("Apply color to all keys");
    fill.set_tooltip_text(Some("Replace all saved key colors with the current swatch"));
    fill.set_halign(gtk::Align::End);
    fill.set_sensitive(false);
    panel.append(&fill);
    panel.append(&status);
    let provider = gtk::CssProvider::new();
    for (position, key) in grid.buttons.iter() {
        key.set_widget_name(&format!("rgb-{}-{}", position.row(), position.column()));
        key.style_context()
            .add_provider(&provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION + 1);
    }
    fill.connect_map(|_| lighting_trace::event(format_args!("apply-all mapped")));
    fill.connect_unmap(|_| lighting_trace::event(format_args!("apply-all unmapped")));
    let editor = Rc::new(ColorEditor {
        changes: changes.clone(),
        visible_keys: grid.buttons.clone(),
        provider,
        color,
        brightness,
        slot,
        status,
        fill,
        palettes: Cell::new(palettes),
        painted: Cell::new(None),
        reactive_active: Cell::new(false),
        wallpaper_frame: Cell::new(None),
        current: Cell::new(None),
        loading: Cell::new(false),
        selecting: Cell::new(false),
        area: grid.area.downgrade(),
        selection_changes: RefCell::new(Vec::new()),
        original_colors: RefCell::new(SelectionColors::default()),
    });
    editor.draw();
    let owned = editor.clone();
    panel.connect_destroy(move |_| {
        let _ = &owned;
    });
    let weak = Rc::downgrade(&editor);
    let pane = panel.downgrade();
    readbacks.on_snapshot(move |snapshot| {
        let (Some(editor), Some(panel)) = (weak.upgrade(), pane.upgrade()) else {
            return;
        };
        if editor.changes.pending(Region::Keyboard) {
            lighting_trace::event(format_args!(
                "editor skipped snapshot while change is pending"
            ));
            return;
        }
        if let Some(palettes) = snapshot.palettes
            && editor.palettes.replace(palettes) != palettes
        {
            editor.original_colors.borrow_mut().clear();
            editor.draw();
        }
        let Some((modes, effect)) = &snapshot.lighting else {
            lighting_trace::event(format_args!(
                "editor hides apply-all: missing lighting readback"
            ));
            editor.brightness.set_sensitive(false);
            editor.fill.set_sensitive(false);
            editor
                .status
                .set_label("Couldn’t read the keyboard colors.");
            return;
        };
        let supported = modes.modes.contains(&10);
        lighting_trace::event(format_args!(
            "editor snapshot mode={} custom-supported={supported}",
            effect.mode
        ));
        panel.set_visible(supported);
        editor.color.widget().set_visible(supported);
        editor.fill.set_sensitive(supported);
        editor.loading.set(true);
        let limit = u16::from(modes.settings[0]) + 1;
        editor
            .brightness
            .set_range(1.0, f64::from(limit.max(effect.brightness)));
        editor.loading.set(false);
        editor.load(*effect, false);
    });
    let weak = Rc::downgrade(&editor);
    changes.listen(move |region, custom, actual| {
        let Some(editor) = weak.upgrade().filter(|_| region == Region::Keyboard) else {
            return;
        };
        if let Some(effect) = actual {
            if custom && effect.mode == 10 {
                editor.current.set(Some(effect));
            }
            if !editor.changes.pending(region) {
                editor.load(effect, custom);
            }
        }
    });
    let weak = Rc::downgrade(&editor);
    editor.slot.connect_changed(move |_| {
        if let Some(editor) = weak.upgrade() {
            editor.selection_changes.borrow_mut().clear();
            editor.original_colors.borrow_mut().clear();
            editor.draw();
            editor.show_palette_status();
        }
    });
    let weak = Rc::downgrade(&editor);
    editor.color.connect_color_set(move |color| {
        if let Some(editor) = weak.upgrade() {
            editor.set_brush(color);
        }
    });
    let weak = Rc::downgrade(&editor);
    editor.brightness.connect_value_changed(move |_| {
        if let Some(editor) = weak.upgrade() {
            editor.commit_brightness();
        }
    });
    let weak = Rc::downgrade(&editor);
    editor.fill.connect_clicked(move |_| {
        if let Some(editor) = weak.upgrade() {
            lighting_trace::event(format_args!(
                "apply-all clicked; swatch={:?}",
                editor.color.rgb()
            ));
            let positions: Vec<_> = editor.visible_keys.iter().map(|(key, _)| *key).collect();
            if let Some(frame) =
                edited_frame(None, &positions, positions.len(), Some(editor.color.rgb()))
            {
                editor.send(frame);
            }
        }
    });
    for (position, btn) in editor.visible_keys.iter() {
        let position = *position;
        let weak = Rc::downgrade(&editor);
        btn.connect_toggled(move |key| {
            let Some(editor) = weak.upgrade() else { return };
            if editor.lighting_is_visible() {
                lighting_trace::event(format_args!(
                    "key {position:?} selected={}",
                    key.is_active()
                ));
                editor
                    .selection_changes
                    .borrow_mut()
                    .push((position, key.is_active().then(|| editor.color.rgb())));
            } else if !key.is_active() {
                editor.original_colors.borrow_mut().forget(position);
            }
            if !editor.selecting.replace(true) {
                let weak = Rc::downgrade(&editor);
                glib::idle_add_local_once(move || {
                    if let Some(editor) = weak.upgrade() {
                        editor.selecting.set(false);
                        editor.apply_selection_colors();
                    }
                });
            }
        });
    }
    (panel, editor)
}

impl ColorEditor {
    fn active_slot(&self) -> u8 {
        self.slot.active().unwrap_or_default() as u8
    }

    fn frame(&self) -> Option<RgbFrame> {
        self.palettes.get()[usize::from(self.active_slot())].or_else(|| {
            self.current
                .get()
                .filter(|effect| effect.mode == 0)
                .map(|effect| [[effect.foreground; KEY_COLUMNS]; KEY_ROWS])
        })
    }

    fn set_palette(&self, slot: u8, frame: Option<RgbFrame>) {
        let mut palettes = self.palettes.get();
        palettes[usize::from(slot)] = frame;
        self.palettes.set(palettes);
    }

    fn show_palette_status(&self) {
        self.status.set_label(if self.frame().is_some() {
            "Select keys to paint with the current swatch."
        } else {
            "Showing a red preview. Apply a color to all keys to start a saved palette."
        });
    }

    fn load(&self, effect: Effect, own_write: bool) {
        let previous = self.current.replace(Some(effect));
        if !own_write && previous.is_some_and(|last| last.mode != effect.mode) {
            self.original_colors.borrow_mut().clear();
        }
        lighting_trace::event(format_args!(
            "editor loaded mode={} previous={:?} custom-write={own_write} palette-known={}",
            effect.mode,
            previous.map(|effect| effect.mode),
            self.frame().is_some()
        ));
        self.loading.set(true);
        self.brightness.set_value(effect.brightness.into());
        self.loading.set(false);
        self.brightness.set_sensitive(true);
        self.draw();
        if !own_write {
            self.show_palette_status();
        }
    }

    fn lighting_is_visible(&self) -> bool {
        self.area
            .upgrade()
            .is_some_and(|area| area.has_css_class("lighting-preview") && area.is_mapped())
    }

    fn apply_selection_colors(self: &Rc<Self>) {
        let changes = self.selection_changes.take();
        if changes.is_empty()
            || self.reactive_active.get()
            || !self.lighting_is_visible()
            || !self.brightness.is_sensitive()
        {
            return;
        }
        let Some(previous) = self.frame() else {
            if changes.iter().any(|(_, color)| color.is_some()) {
                lighting_trace::event(format_args!(
                    "brush skipped: unknown palette; selection={changes:?}"
                ));
                self.status.set_label(
                    "Showing a red preview. Apply a color to all keys to start a saved palette.",
                );
            }
            return;
        };
        let mut frame = previous;
        self.original_colors
            .borrow_mut()
            .apply(&mut frame, &changes);
        if frame != previous {
            lighting_trace::event(format_args!("brush selection changed {changes:?}"));
            self.send(frame);
        }
    }

    fn commit_brightness(self: &Rc<Self>) {
        if self.loading.get() || self.current.get().is_none() {
            return;
        }
        let level = self.brightness.value().round() as u16;
        let weak = Rc::downgrade(self);
        self.changes
            .queue(Region::Keyboard, Change::Brightness(level), move |result| {
                if let Some(editor) = weak.upgrade()
                    && !result.is_ok_and(|effect| effect.brightness == level)
                {
                    editor
                        .status
                        .set_label("Couldn’t confirm the keyboard brightness.");
                }
            });
    }

    fn draw(&self) {
        use std::fmt::Write as _;

        let mut css = String::new();
        let frame = self
            .wallpaper_frame
            .get()
            .or_else(|| self.palettes.get()[usize::from(self.active_slot())])
            .unwrap_or([[[255, 0, 0]; KEY_COLUMNS]; KEY_ROWS]);
        if self.painted.replace(Some(frame)) == Some(frame) {
            return;
        }
        for (position, _) in self.visible_keys.iter() {
            let [r, g, b] = frame[usize::from(position.row())][usize::from(position.column())];
            let tint = format!("#{r:02x}{g:02x}{b:02x}");
            let _ = writeln!(
                css,
                ".lighting-preview #rgb-{}-{} {{ background-image: linear-gradient(alpha({tint}, 0.42), alpha({tint}, 0.42)); }}",
                position.row(),
                position.column(),
            );
        }
        self.provider.load_from_data(&css);
    }

    fn set_brush(&self, color: [u8; 3]) {
        let known = self.frame().is_some();
        lighting_trace::event(format_args!("brush swatch={color:?} palette-known={known}"));
        self.status.set_label(if known {
            "Swatch ready. Select more keys to paint."
        } else {
            "Swatch ready. Apply it to all keys once to start painting."
        });
    }

    fn send(self: &Rc<Self>, frame: RgbFrame) {
        let slot = self.active_slot();
        let brightness = self.brightness.value().round() as u16;
        self.set_palette(slot, Some(frame));
        self.draw();
        self.status.set_label("Changing key colors…");
        let weak = Rc::downgrade(self);
        self.changes.queue(
            Region::Keyboard,
            Change::Colors {
                slot,
                brightness,
                frame: Box::new(frame),
            },
            move |result| {
                let Some(editor) = weak.upgrade() else { return };
                if editor.changes.queued_colors(slot) {
                    return;
                }
                let sent = result.is_ok();
                if !sent {
                    editor.set_palette(slot, None);
                }
                if editor.active_slot() == slot {
                    if !sent {
                        editor.original_colors.borrow_mut().clear();
                    }
                    editor.draw();
                    editor.status.set_label(if sent {
                        "Colors sent to the keyboard."
                    } else {
                        "Couldn’t confirm the new colors. Reconnect or apply a new palette."
                    });
                }
            },
        );
    }
}

#[derive(Default)]
struct SelectionColors {
    original: HashMap<Position, [u8; 3]>,
}

impl SelectionColors {
    fn clear(&mut self) {
        self.original.clear();
    }

    fn forget(&mut self, key: Position) {
        self.original.remove(&key);
    }

    fn apply(&mut self, frame: &mut RgbFrame, changes: &[SelectionChange]) {
        for &(key, color) in changes {
            let pixel = &mut frame[usize::from(key.row())][usize::from(key.column())];
            if let Some(brush) = color {
                self.original.entry(key).or_insert(*pixel);
                *pixel = brush;
            } else if let Some(color) = self.original.remove(&key) {
                *pixel = color;
            }
        }
    }
}

fn edited_frame(
    known: Option<RgbFrame>,
    selected: &[Position],
    visible_count: usize,
    color: Option<[u8; 3]>,
) -> Option<RgbFrame> {
    let Some(color) = color else { return known };
    if selected.is_empty() || (known.is_none() && selected.len() != visible_count) {
        return None;
    }
    let mut frame = known.unwrap_or([[[0; 3]; KEY_COLUMNS]; KEY_ROWS]);
    for position in selected {
        frame[usize::from(position.row())][usize::from(position.column())] = color;
    }
    Some(frame)
}

fn wireless_panel(tasks: &Rc<Tasks>) -> gtk::Box {
    let panel = panel("Live 2.4 GHz lighting");
    let detail = label(
        "Play the current effect live over the wireless receiver.",
        "muted",
    );
    detail.set_wrap(true);
    panel.append(&detail);
    let start = primary("Start live lighting");
    start.set_halign(gtk::Align::End);
    panel.append(&start);
    let runner = tasks.clone();
    start.connect_clicked(move |_| {
        let done = runner.clone();
        runner.start(
            "Streaming 2.4 GHz lighting…",
            true,
            |device, cancel| {
                device
                    .keyboard()?
                    .stream_wireless_lighting(|| cancel.load(Ordering::Relaxed))
            },
            move |result| match result {
                Ok(()) => done.message("Live lighting stopped", false),
                Err(error) => done.error(error),
            },
        );
    });
    panel
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direction_choices_are_restricted_by_effect() {
        let keyboard = Region::Keyboard;
        assert_eq!(direction_options(keyboard, 2), &[0, 1, 2, 3, 4, 5]);
        assert_eq!(direction_options(keyboard, 4), &[6, 7]);
        assert_eq!(direction_options(keyboard, 7), &[4, 5]);
        assert_eq!(direction_options(keyboard, 12), &[2, 3]);
        assert_eq!(direction_label(Some(15), 6), ("→", "Left to right"));
        assert_eq!(direction_label(Some(15), 7), ("←", "Right to left"));
        for mode in [0, 1, 3, 6, 8, 9, 10, 11, 14, 16] {
            assert!(direction_options(keyboard, mode).is_empty());
        }
        assert!(direction_options(Region::Side, 2).is_empty());
        assert_eq!(display_direction(keyboard, 4, 1), Some(7));
        assert_eq!(display_direction(keyboard, 4, 3), None);
        assert_eq!(display_direction(keyboard, 2, 255), None);
    }

    #[test]
    fn edits_preserve_known_keys_and_refuse_to_replace_unknown_colors() {
        let first = Position::new(0, 0).unwrap();
        let second = Position::new(0, 1).unwrap();
        let green = Some([0, 255, 0]);
        assert!(edited_frame(None, &[first], 2, green).is_none());
        assert!(edited_frame(None, &[], 2, None).is_none());
        let known = edited_frame(None, &[first, second], 2, green);
        let after = edited_frame(known, &[first], 2, Some([255, 0, 0])).unwrap();
        assert_eq!(after[0][0], [255, 0, 0]);
        assert_eq!(after[0][1], [0, 255, 0]);
        assert_eq!(edited_frame(Some(after), &[], 2, None), Some(after));
    }

    #[test]
    fn effect_readback_must_confirm_the_palette_and_fixed_color() {
        for (region, mode) in [(Region::Keyboard, 2), (Region::Side, 3)] {
            let rainbow = Effect {
                mode,
                brightness: 4,
                speed: 2,
                foreground: [1, 2, 3],
                background: [0; 3],
                direction: 0,
                effect_type: 1,
                open: 1,
            };
            let fixed = Effect {
                foreground: [244, 1, 88],
                effect_type: 0,
                ..rainbow
            };
            let request = region.effect(fixed, 0).unwrap();
            assert_eq!(&request[9..12], &fixed.foreground);
            assert_eq!(request[16], 0);
            assert!(!effect_matches(region, rainbow, fixed, rainbow));
            assert!(!effect_matches(
                region,
                rainbow,
                fixed,
                Effect {
                    foreground: rainbow.foreground,
                    ..fixed
                }
            ));
            assert!(effect_matches(region, rainbow, fixed, fixed));
        }
    }

    #[test]
    fn selection_captures_each_brush_color_and_deselection_restores_each_original() {
        let first = Position::new(0, 0).unwrap();
        let second = Position::new(0, 1).unwrap();
        let mut frame = [[[0; 3]; KEY_COLUMNS]; KEY_ROWS];
        frame[0][0] = [3, 2, 1];
        frame[0][1] = [7, 8, 9];
        let original = frame;
        let mut colors = SelectionColors::default();
        colors.apply(
            &mut frame,
            &[(first, Some([255, 0, 0])), (second, Some([0, 255, 0]))],
        );
        assert_eq!(&frame[0][..2], &[[255, 0, 0], [0, 255, 0]]);
        colors.apply(&mut frame, &[(first, None)]);
        assert_eq!(frame[0][0], original[0][0]);
        assert_eq!(frame[0][1], [0, 255, 0]);
        colors.apply(
            &mut frame,
            &[(first, Some([5; 3])), (first, None), (second, None)],
        );
        assert_eq!(frame, original);
    }
}
