use std::io;
use std::rc::Rc;
use std::sync::Arc;

use dragon_driver::keyboard::advanced;
use dragon_driver::witmod_sdk::keymap::KeyMap;
use dragon_driver::witmod_sdk::profile::{
    AdvancedAction, AdvancedItem, AdvancedKind, MacroEvent, MacroInfo,
};
use gtk::{Orientation, prelude::*};

use super::{Editor, caption, field, label};

const OPERATIONS: &[(&str, &str)] = &[
    ("default", "Default"),
    ("0", "Key or disabled"),
    ("2", "Key combination"),
    ("3", "Media"),
    ("4", "Secondary action"),
    ("9", "Shortcut"),
    ("1", "Macro"),
    ("6", "Paste text"),
    ("8", "Launch application"),
    ("7", "Switch profile"),
    ("dks", "Dynamic keystroke (DKS)"),
    ("tap", "Mod-Tap"),
    ("toggle", "Toggle"),
    ("other", "Lighting control"),
    ("45", "Classic gamepad"),
    ("46", "Xbox 360 gamepad"),
];

const OTHER: &[(&str, &str, &[&str])] = &[
    (
        "20",
        "Keyboard effect",
        &["Overall", "Interactive", "All", "Static"],
    ),
    ("21", "Keyboard brightness", &["Increase", "Decrease"]),
    ("22", "Keyboard speed", &["Increase", "Decrease"]),
    ("23", "Keyboard direction", &["Forward", "Back", "Cycle"]),
    (
        "24",
        "Keyboard color",
        &["Full color", "Change foreground", "Change background"],
    ),
    ("25", "Keyboard custom slot", &["Custom 1", "Custom 2"]),
    ("27", "Lighting recording", &["Record"]),
    ("30", "Side effect", &["Overall", "Interactive", "All"]),
    ("31", "Side brightness", &["Increase", "Decrease"]),
    ("32", "Side speed", &["Increase", "Decrease"]),
    (
        "33",
        "Side color",
        &["Full color", "Change foreground", "Change background"],
    ),
];

const MEDIA: &[(u8, &str)] = &[
    (0, "Volume up"),
    (1, "Volume down"),
    (2, "Mute"),
    (3, "Play / pause"),
    (4, "Next track"),
    (5, "Previous track"),
    (6, "Stop"),
    (8, "Media player"),
    (9, "Calculator"),
    (10, "Computer"),
    (11, "Browser"),
    (12, "Mail"),
    (13, "Screen brightness up"),
    (14, "Screen brightness down"),
    (15, "Search"),
];

pub(super) fn operation_name(code: &str) -> &str {
    if code == "40" {
        return "Advanced key";
    }
    OPERATIONS
        .iter()
        .copied()
        .chain(OTHER.iter().map(|&(id, name, _)| (id, name)))
        .find(|(id, _)| *id == code)
        .map_or("Vendor action", |(_, name)| name)
}

pub(super) fn dialog(editor: &Rc<Editor>) {
    let (Some(window), Some(index), Some(code)) = (
        editor.window.upgrade(),
        editor.active_profile(),
        editor.selected_key(),
    ) else {
        return;
    };
    let dialog = gtk::Dialog::builder()
        .title(format!("Assign {}", caption(&KeyMap::default(), code)))
        .transient_for(&window)
        .modal(true)
        .default_width(520)
        .build();
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    let save = dialog.add_button("Save assignment", gtk::ResponseType::Accept);
    save.add_css_class("suggested-action");
    let content = dialog.content_area();
    content.set_spacing(10);
    for set in [
        gtk::Widget::set_margin_top,
        gtk::Widget::set_margin_bottom,
        gtk::Widget::set_margin_start,
        gtk::Widget::set_margin_end,
    ] {
        set(content.upcast_ref(), 16);
    }
    let kind = gtk::ComboBoxText::new();
    for &(id, name) in OPERATIONS {
        let supported = match id {
            "45" | "46" => editor.gamepad == Some(id),
            "dks" | "tap" | "toggle" => editor.layout.magnetic(),
            _ => true,
        };
        if supported {
            kind.append(Some(id), name);
        }
    }
    content.append(&field("Action", &kind));
    let pages = gtk::Stack::new();
    pages.set_transition_type(gtk::StackTransitionType::Crossfade);
    pages.set_transition_duration(140);
    pages.set_vhomogeneous(false);
    pages.add_named(
        &label("Restore the key's original action.", "muted"),
        Some("default"),
    );
    let direct = key_picker();
    pages.add_named(&field("Send key", &direct), Some("0"));
    let combo_key = key_picker();
    let modifiers = ["Ctrl", "Shift", "Alt", "Super"].map(gtk::CheckButton::with_label);
    let combo = gtk::Box::new(Orientation::Vertical, 6);
    let toggles = gtk::Box::new(Orientation::Horizontal, 8);
    for option in &modifiers {
        toggles.append(option);
    }
    combo.append(&field("Modifiers", &toggles));
    combo.append(&field("Key", &combo_key));
    pages.add_named(&combo, Some("2"));
    let media = gtk::ComboBoxText::new();
    for &(id, name) in MEDIA {
        media.append(Some(&id.to_string()), name);
    }
    media.set_active(Some(0));
    pages.add_named(&field("Media action", &media), Some("3"));
    let secondary = numbered(&[
        "Left click",
        "Middle click",
        "Right click",
        "Scroll up",
        "Scroll down",
        "Forward button",
        "Back button",
    ]);
    pages.add_named(&field("Mouse action", &secondary), Some("4"));
    let shortcuts = numbered(&[
        "Alt + F4",
        "Alt + Tab",
        "Super + D",
        "Super + E",
        "Super + L",
        "Ctrl + C",
        "Ctrl + X",
        "Ctrl + V",
        "Ctrl + F",
        "Ctrl + N",
        "Ctrl + P",
        "Ctrl + S",
    ]);
    pages.add_named(&field("Shortcut", &shortcuts), Some("9"));
    let text = gtk::TextView::new();
    text.set_wrap_mode(gtk::WrapMode::WordChar);
    let text_scroll = gtk::ScrolledWindow::builder()
        .min_content_height(110)
        .child(&text)
        .build();
    let text_box = gtk::Box::new(Orientation::Vertical, 6);
    text_box.append(&label(
        if super::desktop::native_x11() {
            "Paste this text into the focused application."
        } else {
            "Use the Copy text button in the status bar to take the text to the clipboard."
        },
        "muted",
    ));
    text_box.append(&text_scroll);
    pages.add_named(&text_box, Some("6"));

    let app_id = Rc::new(std::cell::RefCell::new(String::new()));
    let app_name = label("No application selected", "muted");
    let app_button = gtk::Button::with_label("Choose application…");
    let app_row = gtk::Box::new(Orientation::Horizontal, 8);
    app_row.append(&app_name);
    app_row.append(&app_button);
    pages.add_named(&app_row, Some("8"));
    {
        let window = dialog.upcast_ref::<gtk::Window>().downgrade();
        let tasks = editor.tasks.clone();
        let id = app_id.clone();
        let name = app_name.clone();
        app_button.connect_clicked(move |_| {
            let id = id.clone();
            let name = name.clone();
            super::desktop::choose_application(&tasks, &window, move |app| {
                *id.borrow_mut() = app.id;
                name.set_label(&app.name);
            });
        });
    }
    let profile = gtk::ComboBoxText::new();
    for item in editor.draft.profiles.borrow().list_profiles() {
        profile.append(Some(item.index), item.name);
    }
    profile.set_active(Some(0));
    pages.add_named(&field("Switch to", &profile), Some("7"));
    let macro_editor = MacroEditor::new();
    pages.add_named(&macro_editor.view, Some("1"));
    let dks = DksEditor::new();
    pages.add_named(&dks.view, Some("dks"));
    let tap = gtk::Box::new(Orientation::Vertical, 6);
    let tap_keys = [key_picker(), key_picker()];
    let tap_time = gtk::SpinButton::with_range(10.0, 2550.0, 10.0);
    tap_time.set_value(200.0);
    tap.append(&field("Tap output", &tap_keys[0]));
    tap.append(&field("Hold output", &tap_keys[1]));
    tap.append(&field("Hold after · ms", &tap_time));
    pages.add_named(&tap, Some("tap"));
    let toggle = key_picker();
    pages.add_named(&field("Toggle key", &toggle), Some("toggle"));
    let classic = numbered(&[
        "X axis left",
        "X axis right",
        "Y axis up",
        "Y axis down",
        "Z axis forward",
        "Z axis backward",
        "Z rotation forward",
        "Z rotation backward",
        "X rotation left",
        "X rotation right",
        "Y rotation up",
        "Y rotation down",
        "Throttle",
        "Elevator",
        "Accelerator",
        "Rudder",
        "Trigger",
        "Button 1",
        "Button 2",
        "Button 3",
        "Button 4",
        "Button 5",
        "Button 6",
        "Button 7",
        "Button 8",
        "Button 9",
        "Button 10",
        "Button 11",
        "Button 12",
        "D-pad up",
        "D-pad down",
        "D-pad left",
        "D-pad right",
    ]);
    let xbox = numbered(&[
        "Left stick up",
        "Left stick down",
        "Left stick left",
        "Left stick right",
        "Right stick up",
        "Right stick down",
        "Right stick left",
        "Right stick right",
        "Left trigger",
        "Right trigger",
        "A",
        "B",
        "X",
        "Y",
        "Start",
        "Select",
        "Menu",
        "D-pad up",
        "D-pad down",
        "D-pad left",
        "D-pad right",
        "Left stick press",
        "Right stick press",
        "Left bumper",
        "Right bumper",
    ]);
    pages.add_named(&field("Classic gamepad output", &classic), Some("45"));
    pages.add_named(&field("Xbox 360 gamepad output", &xbox), Some("46"));
    let other = gtk::ComboBoxText::new();
    for &(id, name, _) in OTHER {
        if editor.layout.side_lighting || !matches!(id, "30" | "31" | "32" | "33") {
            other.append(Some(id), name);
        }
    }
    let parameter = gtk::ComboBoxText::new();
    let weak = parameter.downgrade();
    other.connect_changed(move |combo| {
        let Some(parameter) = weak.upgrade() else {
            return;
        };
        parameter.remove_all();
        let id = combo.active_id();
        if let Some((_, _, choices)) = OTHER
            .iter()
            .find(|(code, _, _)| Some(*code) == id.as_deref())
        {
            for (index, choice) in choices.iter().enumerate() {
                parameter.append(Some(&index.to_string()), choice);
            }
            parameter.set_active(Some(0));
        }
    });
    other.set_active(Some(0));
    let other_panel = gtk::Box::new(Orientation::Vertical, 6);
    other_panel.append(&field("Lighting control", &other));
    other_panel.append(&field("Action", &parameter));
    pages.add_named(&other_panel, Some("other"));
    content.append(&pages);
    let feedback = label("", "error");
    feedback.set_wrap(true);
    content.append(&feedback);
    let weak = pages.downgrade();
    kind.connect_changed(move |kind| {
        if let (Some(pages), Some(id)) = (weak.upgrade(), kind.active_id()) {
            pages.set_visible_child_name(&id);
        }
    });
    kind.set_active_id(Some("default"));

    let old_advanced = {
        let profiles = editor.draft.profiles.borrow();
        let code = code.to_string();
        let current = profiles
            .key_actions(&index)
            .and_then(|mut actions| actions.find(|action| action.key_code == code));
        let mut old = None;
        if let Some(action) = current {
            match action.operation_type {
                "0" => {
                    direct.set_active_id(Some(action.value));
                }
                "1" => macro_editor.load(&profiles, &index, &action),
                "2" => {
                    if let Some((mask, key)) = action.value.split_once(',') {
                        combo_key.set_active_id(Some(key));
                        let mask = mask.parse::<u8>().unwrap_or(0);
                        for (bit, check) in modifiers.iter().enumerate() {
                            check.set_active(mask & (1 << bit) != 0);
                        }
                    }
                }
                "3" => {
                    media.set_active_id(Some(action.value));
                }
                "4" => {
                    secondary.set_active_id(Some(action.value));
                }
                "9" => {
                    shortcuts.set_active_id(Some(action.value));
                }
                "6" => text.buffer().set_text(action.value),
                "8" => {
                    *app_id.borrow_mut() = action.value.into();
                    app_name.set_label(action.value);
                }
                "7" => {
                    profile.set_active_id(Some(action.value));
                }
                "45" => {
                    classic.set_active_id(Some(action.value));
                }
                "46" => {
                    xbox.set_active_id(Some(action.value));
                }
                "40" => {
                    old = action.value.parse::<i32>().ok();
                    let definitions = editor.draft.advanced.borrow();
                    for group in [
                        AdvancedKind::Dks,
                        AdvancedKind::ModTap,
                        AdvancedKind::Toggle,
                    ] {
                        if let Some(item) = definitions
                            .advanced_items(group)
                            .unwrap_or_default()
                            .into_iter()
                            .find(|item| Some(item.index) == old)
                        {
                            match item.action {
                                AdvancedAction::Dks {
                                    key_codes,
                                    stages,
                                    values,
                                } => {
                                    dks.load(key_codes, stages, values);
                                    kind.set_active_id(Some("dks"));
                                }
                                AdvancedAction::ModTap {
                                    key_codes,
                                    value_time,
                                } => {
                                    for (picker, code) in tap_keys.iter().zip(key_codes) {
                                        picker.set_active_id(Some(code));
                                    }
                                    tap_time.set_value(value_time.into());
                                    kind.set_active_id(Some("tap"));
                                }
                                AdvancedAction::Toggle { key_code, .. } => {
                                    toggle.set_active_id(Some(key_code));
                                    kind.set_active_id(Some("toggle"));
                                }
                            }
                        }
                    }
                }
                operation => {
                    if other.set_active_id(Some(operation)) {
                        parameter.set_active_id(Some(action.value));
                        kind.set_active_id(Some("other"));
                    }
                }
            }
            if matches!(
                action.operation_type,
                "0" | "1" | "2" | "3" | "4" | "6" | "7" | "8" | "9"
            ) || editor.gamepad == Some(action.operation_type)
            {
                kind.set_active_id(Some(action.operation_type));
            }
        }
        old
    };

    let weak = Rc::downgrade(editor);
    dialog.connect_response(move |dialog, response| {
        if response != gtk::ResponseType::Accept {
            dialog.close();
            return;
        }
        let Some(editor) = weak.upgrade() else {
            dialog.close();
            return;
        };
        let result = (|| -> io::Result<()> {
            let kind = select(&kind)?;
            let usage = |key: &gtk::ComboBoxText| select(key).map(|value| value.to_string());
            let store = |kind, parameter: String| {
                editor.save_assignment(&index, code, kind, &parameter, "1", "1")
            };
            match kind.as_str() {
                "default" => {
                    let code = code.to_string();
                    let mut document = editor.draft.profiles.borrow_mut();
                    while Arc::make_mut(&mut document).remove_key_action(&index, &code) {}
                    drop(document);
                    editor.update_selection();
                }
                "0" => store("0", usage(&direct)?),
                "2" => {
                    let mask = modifiers
                        .iter()
                        .enumerate()
                        .filter(|(_, check)| check.is_active())
                        .map(|(bit, _)| 1_u8 << bit)
                        .sum::<u8>();
                    if !(1..=2).contains(&mask.count_ones()) {
                        return Err(invalid("Select one or two modifiers."));
                    }
                    let key = usage(&combo_key)?;
                    key.parse::<u8>()
                        .map_err(|_| invalid("Select a valid combination key."))?;
                    store("2", format!("{mask},{key}"));
                }
                "3" => store("3", usage(&media)?),
                "4" => store("4", usage(&secondary)?),
                "9" => store("9", usage(&shortcuts)?),
                "45" => store("45", usage(&classic)?),
                "46" => store("46", usage(&xbox)?),
                "6" => {
                    let buffer = text.buffer();
                    let value = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
                    if value.is_empty() || value.len() > 5119 {
                        return Err(invalid("Enter text up to 5,119 UTF-8 bytes."));
                    }
                    store("6", value.to_string());
                }
                "8" => {
                    let id = app_id.borrow();
                    if !super::desktop::valid_id(&id) {
                        return Err(invalid("Choose an installed Linux application."));
                    }
                    store("8", id.to_owned());
                }
                "7" => store("7", usage(&profile)?),
                "1" => macro_editor.store(&editor, &index, code)?,
                "dks" => {
                    let key_codes = dks
                        .keys
                        .each_ref()
                        .map(|key| key.active_id().unwrap_or_default());
                    let values = dks.travel.each_ref().map(gtk::SpinButton::value_as_int);
                    let stages = dks
                        .stages
                        .each_ref()
                        .map(|row| row.each_ref().map(|cell| i32::from(cell.is_active())));
                    let action = AdvancedAction::Dks {
                        key_codes: key_codes.each_ref().map(|code| code.as_str()),
                        stages,
                        values,
                    };
                    store_advanced(
                        &editor,
                        &index,
                        code,
                        old_advanced,
                        "Dynamic keystroke",
                        action,
                    )?;
                }
                "tap" => {
                    let tap = usage(&tap_keys[0])?;
                    let hold = usage(&tap_keys[1])?;
                    let action = AdvancedAction::ModTap {
                        key_codes: [&tap, &hold],
                        value_time: tap_time.value_as_int(),
                    };
                    store_advanced(&editor, &index, code, old_advanced, "Mod-Tap", action)?;
                }
                "toggle" => {
                    let key = usage(&toggle)?;
                    store_advanced(
                        &editor,
                        &index,
                        code,
                        old_advanced,
                        "Toggle",
                        AdvancedAction::Toggle {
                            key_code: &key,
                            status: 1,
                        },
                    )?;
                }
                "other" => store(&select(&other)?, usage(&parameter)?),
                _ => return Err(invalid("Choose an assignment.")),
            }
            Ok(())
        })();
        match result {
            Ok(()) => dialog.close(),
            Err(error) => feedback.set_label(&error.to_string()),
        }
    });
    dialog.present();
}

fn select(combo: &gtk::ComboBoxText) -> io::Result<gtk::glib::GString> {
    combo.active_id().ok_or_else(|| invalid("Choose a value."))
}

fn numbered(names: &[&str]) -> gtk::ComboBoxText {
    let options = gtk::ComboBoxText::new();
    for (i, name) in names.iter().enumerate() {
        options.append(Some(&i.to_string()), name);
    }
    options.set_active(Some(0));
    options
}

fn key_picker() -> gtk::ComboBoxText {
    let map = KeyMap::default();
    let combo = gtk::ComboBoxText::new();
    combo.append(Some("0"), "No key");
    combo.append(Some("-20"), "Disabled");
    for usage in 4..=231 {
        if let Some(name) = map.label(usage) {
            combo.append(Some(&usage.to_string()), name);
        }
    }
    combo.set_active_id(Some("4"));
    combo
}

struct DksEditor {
    view: gtk::Box,
    keys: [gtk::ComboBoxText; 4],
    travel: [gtk::SpinButton; 4],
    stages: [[gtk::CheckButton; 4]; 4],
}

impl DksEditor {
    fn new() -> Self {
        let view = gtk::Box::new(Orientation::Vertical, 7);
        view.append(&label(
            "Set travel points (in 0.1 mm steps) and choose the keys triggered at each point.",
            "muted",
        ));
        let keys = std::array::from_fn(|i| {
            let key = key_picker();
            key.set_active_id(Some(if i == 0 { "4" } else { "0" }));
            key
        });
        for (i, key) in keys.iter().enumerate() {
            view.append(&field(&format!("Output {}", i + 1), key));
        }
        let travel = [15, 36, 36, 15].map(|value| {
            let spin = gtk::SpinButton::with_range(1.0, 255.0, 1.0);
            spin.set_value(f64::from(value));
            spin
        });
        let stages = std::array::from_fn(|_| std::array::from_fn(|_| gtk::CheckButton::new()));
        for (i, (spin, row)) in travel.iter().zip(&stages).enumerate() {
            let values = gtk::Box::new(Orientation::Horizontal, 7);
            values.append(spin);
            for (index, option) in row.iter().enumerate() {
                option.set_label(Some(&(index + 1).to_string()));
                values.append(option);
            }
            view.append(&field(&format!("Travel point {}", i + 1), &values));
        }
        Self {
            view,
            keys,
            travel,
            stages,
        }
    }

    fn load(&self, keys: [&str; 4], stages: [[i32; 4]; 4], values: [i32; 4]) {
        for (picker, key) in self.keys.iter().zip(keys) {
            picker.set_active_id(Some(key));
        }
        for (spin, value) in self.travel.iter().zip(values) {
            spin.set_value(value.into());
        }
        for (row, values) in self.stages.iter().zip(stages) {
            for (button, value) in row.iter().zip(values) {
                button.set_active(value != 0);
            }
        }
    }
}

struct MacroEditor {
    view: gtk::Box,
    events: gtk::TextView,
    mode: gtk::ComboBoxText,
    repeats: gtk::SpinButton,
    recording: gtk::ToggleButton,
}

impl MacroEditor {
    fn new() -> Self {
        let view = gtk::Box::new(Orientation::Vertical, 6);
        let info = label(
            "Enter one event per line with delay in ms, key or mouse, HID code, and down or up. A sample release is 30 key 4 up.",
            "muted",
        );
        info.set_wrap(true);
        view.append(&info);
        let events = gtk::TextView::new();
        events.set_monospace(true);
        events.buffer().set_text("0 key 4 down\n30 key 4 up");
        let scroll = gtk::ScrolledWindow::builder()
            .min_content_height(110)
            .max_content_height(180)
            .child(&events)
            .build();
        view.append(&scroll);
        let recording = gtk::ToggleButton::with_label("Record replacement");
        recording.set_halign(gtk::Align::Start);
        recording.set_tooltip_text(Some(
            "Type into the recorder. Press Ctrl + Escape or click again to stop.",
        ));
        super::recording::attach(&events, &recording);
        view.append(&recording);
        let mode = gtk::ComboBoxText::new();
        mode.append(Some("1"), "Repeat count");
        mode.append(Some("2"), "While held");
        mode.append(Some("3"), "Toggle");
        mode.set_active(Some(0));
        let repeats = gtk::SpinButton::with_range(1.0, 255.0, 1.0);
        view.append(&field("Playback", &mode));
        view.append(&field("Repeats", &repeats));
        Self {
            view,
            events,
            mode,
            repeats,
            recording,
        }
    }

    fn load(
        &self,
        profiles: &dragon_driver::witmod_sdk::profile::Profiles,
        profile: &str,
        action: &dragon_driver::witmod_sdk::profile::KeyAction<'_>,
    ) {
        if let Some(events) = profiles.bound_macro_events(profile, action.value) {
            let lines = events
                .iter()
                .map(|event| {
                    format!(
                        "{} {} {} {}",
                        event.delay, event.event_type, event.key_code, event.explanation
                    )
                })
                .collect::<Vec<_>>()
                .join("\n");
            self.events.buffer().set_text(&lines);
        }
        self.mode.set_active_id(Some(action.mode));
        self.repeats
            .set_value(action.repeat_count.parse().unwrap_or(1.0));
    }

    fn store(&self, editor: &Editor, profile: &str, code: i16) -> io::Result<()> {
        if self.recording.is_active() {
            self.recording.set_active(false);
        }
        let buffer = self.events.buffer();
        let text = buffer.text(&buffer.start_iter(), &buffer.end_iter(), false);
        let mut records = Vec::new();
        for line in text.lines().filter(|line| !line.trim().is_empty()) {
            let parts: Vec<_> = line.split_whitespace().take(5).collect();
            let &[delay, kind, key, action] = parts.as_slice() else {
                return Err(invalid(
                    "Each macro line needs a delay, event type, HID code, and down or up.",
                ));
            };
            if delay.parse::<u16>().is_err()
                || !matches!(action, "down" | "up")
                || match kind {
                    "key" => key.parse::<u8>().is_err(),
                    "mouse" => !matches!(key, "513" | "514" | "516" | "517" | "519" | "520"),
                    _ => true,
                }
            {
                return Err(invalid(
                    "A macro line contains an invalid delay, key, or direction.",
                ));
            }
            if records.len() == 62 {
                return Err(invalid("Onboard macros support up to 62 events."));
            }
            records.push(MacroEvent {
                delay,
                event_type: kind,
                key_code: key,
                explanation: action,
                mark: "0",
                code_name: key,
            });
        }
        if records.is_empty() {
            return Err(invalid("Add at least one macro event."));
        }
        let mode = select(&self.mode)?;
        let mut document = editor.draft.profiles.borrow_mut();
        let profiles = Arc::make_mut(&mut document);
        let code_str = code.to_string();
        let existing = profiles
            .key_actions(profile)
            .and_then(|mut actions| {
                actions.find(|action| action.key_code == code_str && action.operation_type == "1")
            })
            .map(|action| action.value.to_owned());
        let id = match existing {
            Some(id) => id,
            None => (1..=512)
                .map(|i| i.to_string())
                .find(|id| profiles.bound_macro_events(profile, id).is_none())
                .ok_or_else(|| invalid("No more macro identifiers are available."))?,
        };
        profiles.remove_bound_macro(profile, &id);
        if !profiles.append_bound_macro(
            profile,
            MacroInfo {
                index: &id,
                name: "Key macro",
            },
            &records,
        ) {
            return Err(invalid(
                "This imported profile has no macro storage section.",
            ));
        }
        drop(document);
        editor.save_assignment(
            profile,
            code,
            "1",
            &id,
            &mode,
            &self.repeats.value_as_int().to_string(),
        );
        Ok(())
    }
}

fn store_advanced(
    editor: &Editor,
    profile: &str,
    key: i16,
    _old: Option<i32>,
    name: &str,
    action: AdvancedAction<'_>,
) -> io::Result<()> {
    advanced::encode(0, action)?;
    let mut document = editor.draft.advanced.borrow_mut();
    let definitions = Arc::make_mut(&mut document);
    let index = (1..=1024)
        .find(|&index| !definitions.contains_advanced_index(index))
        .ok_or_else(|| invalid("No more advanced-key identifiers are available."))?;
    if !definitions.append_advanced_item(AdvancedItem {
        index,
        name,
        action,
    }) {
        return Err(invalid(
            "This document has no advanced-key section for that action.",
        ));
    }
    drop(document);
    editor.save_assignment(profile, key, "40", &index.to_string(), "1", "1");
    Ok(())
}

fn invalid(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message)
}
