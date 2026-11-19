use std::cell::{Cell, RefCell};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use dragon_driver::keyboard::{
    bindings::encode_profile, desktop::DesktopBinding, models::Layout, session::UploadResult,
};
use dragon_driver::witmod_sdk::keymap::KeyMap;
use dragon_driver::witmod_sdk::profile::{KeyAction, Profiles};
use gtk::{Orientation, glib, prelude::*};

use super::desktop::{self, Event, Focus, Follow};
use super::keys::{KeyGrid, caption};
use super::worker::{Connected, Tasks};
use super::{confirm, field, label, panel, pick_file, primary};

mod actions;
mod recording;

pub(super) struct Draft {
    profiles: RefCell<Arc<Profiles>>,
    advanced: RefCell<Arc<Profiles>>,
}

impl Draft {
    pub fn new() -> Rc<Self> {
        let mut profiles = Profiles::default();
        profiles.append_profile("1", "Main", false);
        profiles.append_profile("2", "Fn", false);
        Rc::new(Self {
            profiles: RefCell::new(Arc::new(profiles)),
            advanced: RefCell::new(Arc::new(Profiles::advanced_document())),
        })
    }
}

struct Editor {
    draft: Rc<Draft>,
    main: gtk::ComboBoxText,
    function: gtk::ComboBoxText,
    layer: gtk::ComboBoxText,
    buttons: Vec<(i16, gtk::ToggleButton)>,
    selected: gtk::Label,
    edit: gtk::Button,
    remove: gtk::Button,
    tasks: Rc<Tasks>,
    window: glib::WeakRef<gtk::ApplicationWindow>,
    layout: &'static Layout,
    gamepad: Option<&'static str>,
    foreground: RefCell<Option<Focus>>,
    follow: RefCell<Follow>,
    follow_enabled: Cell<bool>,
    active_main: RefCell<Option<String>>,
    transfer_status: gtk::Label,
}

pub(super) fn page(
    device: &Connected,
    grid: &KeyGrid,
    tasks: &Rc<Tasks>,
    window: &gtk::ApplicationWindow,
    draft: Rc<Draft>,
) -> gtk::Box {
    let layout = device.layout;
    let page = super::page("Profiles", "folder-symbolic");
    let workspace = panel("Profile files");
    let main = gtk::ComboBoxText::new();
    let function = gtk::ComboBoxText::new();
    if layout.onboard_selection {
        workspace.append(&field("Main layer profile", &main));
        workspace.append(&field("Fn layer profile", &function));
    }
    let add = gtk::Button::with_label("New profile…");
    let rename = gtk::Button::with_label("Rename…");
    let import = gtk::Button::with_label("Import XML…");
    let export = gtk::Button::with_label("Export XML…");
    let files = gtk::Box::new(Orientation::Horizontal, 8);
    if layout.onboard_selection {
        files.append(&add);
        files.append(&rename);
    }
    files.append(&import);
    files.append(&export);
    workspace.append(&files);
    page.append(&workspace);

    let bindings = panel("Key assignment");
    let layer = gtk::ComboBoxText::new();
    layer.append_text("Main layer");
    layer.append_text("Fn layer");
    layer.set_active(Some(0));
    bindings.append(&field("Editing", &layer));
    let buttons: Vec<_> = grid
        .buttons
        .iter()
        .filter_map(|(position, button)| {
            let code = layout.key(*position)?;
            Some((code, button.clone()))
        })
        .collect();
    let selected = label("Select one key to edit its assignment.", "muted");
    selected.set_wrap(true);
    bindings.append(&selected);
    let edit = gtk::Button::with_label("Edit assignment…");
    let remove = gtk::Button::with_label("Restore selected key");
    let row = gtk::Box::new(Orientation::Horizontal, 8);
    row.set_halign(gtk::Align::End);
    row.append(&remove);
    row.append(&edit);
    bindings.append(&row);
    page.append(&bindings);

    let transfer = panel("Save to keyboard");
    let status = label(
        if layout.onboard_selection {
            "Saves both the Main and Fn layers."
        } else {
            "Saves both layers together."
        },
        "muted",
    );
    status.set_wrap(true);
    transfer.append(&status);
    let row = gtk::Box::new(Orientation::Horizontal, 8);
    row.set_halign(gtk::Align::End);
    let preview = gtk::Button::with_label("Check layout");
    let upload = primary("Write to keyboard…");
    row.append(&preview);
    row.append(&upload);
    transfer.append(&row);
    page.append(&transfer);

    let editor = Rc::new(Editor {
        draft,
        main,
        function,
        layer,
        buttons,
        selected,
        edit,
        remove,
        tasks: tasks.clone(),
        window: window.downgrade(),
        layout,
        foreground: RefCell::new(None),
        follow: RefCell::new(Follow::default()),
        follow_enabled: Cell::new(false),
        active_main: RefCell::new(None),
        transfer_status: status.clone(),
        gamepad: if device.gamepad() {
            match device.usb_id.product {
                0x7473 => Some("46"),
                0x7474 => Some("45"),
                _ => None,
            }
        } else {
            None
        },
    });
    editor.update_choices(Some("1"), Some("2"));
    let owned = editor.clone();
    page.connect_destroy(move |_| {
        drop(owned.clone());
    });

    for select in [&editor.main, &editor.function, &editor.layer] {
        let editor = Rc::downgrade(&editor);
        select.connect_changed(move |_| {
            if let Some(editor) = editor.upgrade() {
                editor.update_selection();
            }
        });
    }
    for (_, button) in &editor.buttons {
        let editor = Rc::downgrade(&editor);
        button.connect_toggled(move |_| {
            if let Some(editor) = editor.upgrade() {
                editor.update_selection();
            }
        });
    }
    let weak = Rc::downgrade(&editor);
    editor.edit.connect_clicked(move |_| {
        if let Some(editor) = weak.upgrade() {
            actions::dialog(&editor);
        }
    });
    let weak = Rc::downgrade(&editor);
    editor.remove.connect_clicked(move |_| {
        let Some(editor) = weak.upgrade() else { return };
        if let (Some(index), Some(code)) = (editor.active_profile(), editor.selected_key()) {
            let code = code.to_string();
            let mut document = editor.draft.profiles.borrow_mut();
            while Arc::make_mut(&mut document).remove_key_action(&index, &code) {}
            drop(document);
            editor.update_selection();
        }
    });
    for (button, create) in [(add, true), (rename, false)] {
        let weak = Rc::downgrade(&editor);
        button.connect_clicked(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.name_dialog(create);
            }
        });
    }
    for (button, is_export) in [(import, false), (export, true)] {
        let weak = Rc::downgrade(&editor);
        button.connect_clicked(move |_| {
            if let Some(editor) = weak.upgrade() {
                editor.file_dialog(is_export);
            }
        });
    }
    let weak = Rc::downgrade(&editor);
    let copy = status.clone();
    preview.connect_clicked(move |_| {
        if let Some(editor) = weak.upgrade() {
            editor.validate(copy.clone(), false);
        }
    });
    let weak = Rc::downgrade(&editor);
    upload.connect_clicked(move |_| {
        if let Some(editor) = weak.upgrade() {
            editor.validate(status.clone(), true);
        }
    });
    if editor.layout.onboard_selection {
        workspace.append(&editor.application_controls());
    }
    editor.watch();
    editor.update_selection();
    page
}

impl Editor {
    fn application_controls(self: &Rc<Self>) -> gtk::Box {
        let controls = gtk::Box::new(Orientation::Vertical, 8);
        let link = gtk::Button::with_label("Link foreground app…");
        let unlink = gtk::Button::with_label("Unlink app");
        let buttons = gtk::Box::new(Orientation::Horizontal, 8);
        buttons.append(&link);
        buttons.append(&unlink);
        controls.append(&buttons);
        let association = label("", "muted");
        association.set_wrap(true);
        controls.append(&association);
        let update = {
            let editor = Rc::downgrade(self);
            let association = association.downgrade();
            move || {
                if let (Some(editor), Some(label)) = (editor.upgrade(), association.upgrade()) {
                    let profile = editor.main.active_id();
                    let profiles = editor.draft.profiles.borrow();
                    let app = profile
                        .as_deref()
                        .and_then(|id| profiles.profile(id))
                        .map(|info| info.application);
                    let text = app.filter(|app| !app.is_empty()).map_or_else(
                        || "Main profile has no linked application".to_owned(),
                        |app| format!("Main profile follows {app}"),
                    );
                    label.set_label(&text);
                }
            }
        };
        update();
        let show = update.clone();
        self.main.connect_changed(move |_| show());
        let editor = Rc::downgrade(self);
        let show = update.clone();
        link.connect_clicked(move |_| {
            let Some(editor) = editor.upgrade() else {
                return;
            };
            let (Some(index), Some(window)) = (editor.main.active_id(), editor.window.upgrade())
            else {
                return;
            };
            let weak = Rc::downgrade(&editor);
            let show = show.clone();
            desktop::choose_application(
                &editor.tasks,
                &window.upcast::<gtk::Window>().downgrade(),
                move |app| {
                    if let Some(editor) = weak.upgrade() {
                        Arc::make_mut(&mut editor.draft.profiles.borrow_mut())
                            .set_profile_application(&index, &app.id, app.class.as_deref());
                        show();
                    }
                },
            );
        });
        let editor = Rc::downgrade(self);
        unlink.connect_clicked(move |_| {
            if let Some(editor) = editor.upgrade()
                && let Some(index) = editor.main.active_id()
            {
                Arc::make_mut(&mut editor.draft.profiles.borrow_mut())
                    .set_profile_application(&index, "", None);
                update();
            }
        });
        if desktop::native_x11() {
            let follow = gtk::CheckButton::with_label(
                "Switch profiles when linked apps come to the foreground",
            );
            follow.set_tooltip_text(Some("Writes the linked Main profile and your chosen Fn profile. Unlinked apps leave the current profile active."));
            controls.append(&follow);
            let editor = Rc::downgrade(self);
            let resetting = Rc::new(Cell::new(false));
            follow.connect_toggled(move |check| {
                if resetting.get() {
                    return;
                }
                let Some(editor) = editor.upgrade() else {
                    return;
                };
                let enabled = check.is_active();
                editor.follow_enabled.set(enabled);
                editor.foreground.borrow_mut().take();
                *editor.follow.borrow_mut() = Follow::default();
                let weak = Rc::downgrade(&editor);
                let check = check.downgrade();
                let resetting = resetting.clone();
                editor.tasks.start(
                    "Connecting to foreground monitoring…",
                    false,
                    move |device, _| device.watch_focus(enabled),
                    move |result| {
                        if let Some(editor) = weak.upgrade() {
                            match result {
                                Ok(()) => editor.tasks.message(
                                    if enabled {
                                        "Foreground profile switching enabled"
                                    } else {
                                        "Foreground profile switching stopped"
                                    },
                                    false,
                                ),
                                Err(error) => {
                                    editor.follow_enabled.set(false);
                                    if let Some(check) = check.upgrade() {
                                        resetting.set(true);
                                        check.set_active(false);
                                        resetting.set(false);
                                    }
                                    editor.tasks.error(error);
                                }
                            }
                        }
                    },
                );
            });
        } else {
            controls.append(&label(
                "Automatic foreground switching requires an X11 session.",
                "muted",
            ));
        }
        controls
    }

    fn watch(self: &Rc<Self>) {
        let weak = Rc::downgrade(self);
        let revision = self.tasks.revision.get();
        glib::timeout_add_local(Duration::from_millis(100), move || {
            let Some(editor) = weak.upgrade() else {
                return glib::ControlFlow::Break;
            };
            if editor.tasks.revision.get() != revision {
                return glib::ControlFlow::Break;
            }
            for _ in 0..12 {
                match editor.tasks.next_event() {
                    Some(Event::Focus(focus)) if editor.follow_enabled.get() => {
                        *editor.foreground.borrow_mut() = focus
                    }
                    Some(Event::Focus(_)) => (),
                    Some(Event::Binding(DesktopBinding::Profile(index), _)) => {
                        editor.activate(index, None)
                    }
                    Some(Event::Binding(action @ DesktopBinding::Text(_), target)) => {
                        editor.tasks.desktop_action(action, target);
                        break;
                    }
                    Some(Event::Binding(action, target)) => {
                        editor.tasks.desktop_action(action, target)
                    }
                    Some(Event::Message(note)) => editor.tasks.message(&note, true),
                    None => break,
                }
            }
            if editor.follow_enabled.get() && !editor.tasks.busy.get() {
                let profile = editor.linked_profile();
                let active = editor.active_main.borrow();
                let chosen = editor.follow.borrow_mut().ready(
                    profile.as_deref(),
                    active.as_deref(),
                    Instant::now(),
                );
                drop(active);
                if let Some(profile) = chosen {
                    let window = editor
                        .foreground
                        .borrow()
                        .as_ref()
                        .map(|focus| focus.window);
                    if let Some(window) = window {
                        editor.activate(profile, Some(window));
                    }
                }
            }
            glib::ControlFlow::Continue
        });
    }

    fn linked_profile(&self) -> Option<String> {
        let foreground = self.foreground.borrow();
        let focus = foreground.as_ref()?;
        let profiles = self.draft.profiles.borrow();
        let mut best = None;
        let mut strength = 0;
        for profile in profiles.list_profiles() {
            let score = desktop::match_strength(
                focus,
                profile.application,
                profiles.profile_x11_class(profile.index),
            );
            if score > strength {
                strength = score;
                best = Some(profile.index.to_owned());
            } else if score == strength && score != 0 {
                best = None;
            }
        }
        best
    }

    fn activate(self: &Rc<Self>, index: String, expected_window: Option<u32>) {
        if self.tasks.busy.get() || self.active_main.borrow().as_deref() == Some(&index) {
            return;
        }
        let Some(function) = self.function.active_id().map(|id| id.to_string()) else {
            return;
        };
        let profiles = self.draft.profiles.borrow().clone();
        let advanced = self.draft.advanced.borrow().clone();
        let layout = self.layout;
        let requested = index.clone();
        let editor = Rc::downgrade(self);
        self.tasks.start(
            "Switching keyboard profile…",
            false,
            move |device, _| {
                let plan = encode_profile(
                    (&profiles, &requested),
                    (&profiles, &function),
                    Some(&advanced),
                    layout,
                )?;
                if expected_window.is_some_and(|window| !device.still_focused(window)) {
                    return Ok(None);
                }
                device.upload(&plan).map(Some)
            },
            move |result| {
                if let Some(editor) = editor.upgrade() {
                    match result {
                        Ok(Some(result)) => {
                            *editor.active_main.borrow_mut() = Some(index.clone());
                            editor.main.set_active_id(Some(&index));
                            editor.upload_status(result);
                        }
                        Ok(None) => (),
                        Err(error) => {
                            editor.foreground.borrow_mut().take();
                            editor.tasks.error(error);
                        }
                    }
                }
            },
        );
    }

    fn upload_status(&self, result: UploadResult) {
        self.transfer_status.set_label(match result {
            UploadResult::DeviceAccepted => "Onboard configuration accepted by the keyboard.",
            UploadResult::Transmitted => "Reports sent. The keyboard gave no final confirmation.",
        });
        self.tasks.message(
            match result {
                UploadResult::DeviceAccepted => "Profile active",
                UploadResult::Transmitted => {
                    "Profile transmitted. Desktop bindings need device confirmation."
                }
            },
            false,
        );
    }

    fn active_profile(&self) -> Option<glib::GString> {
        if self.layer.active() == Some(1) {
            self.function.active_id()
        } else {
            self.main.active_id()
        }
    }

    fn selected_key(&self) -> Option<i16> {
        let mut selected = self.buttons.iter().filter(|(_, button)| button.is_active());
        let (code, _) = selected.next()?;
        selected.next().is_none().then_some(*code)
    }

    fn update_choices(&self, primary: Option<&str>, function: Option<&str>) {
        let profiles = self.draft.profiles.borrow();
        let entries = profiles.list_profiles();
        for (combo, current) in [(&self.main, primary), (&self.function, function)] {
            combo.remove_all();
            for item in &entries {
                combo.append(Some(item.index), item.name);
            }
            if !combo.set_active_id(current) {
                combo.set_active(Some(0));
            }
        }
    }

    fn refresh_choices(&self) {
        self.update_choices(
            self.main.active_id().as_deref(),
            self.function.active_id().as_deref(),
        );
    }

    fn update_selection(&self) {
        let normal = "Select one key to edit its assignment.";
        let mut message = normal.to_string();
        let mut mapped = false;
        if let (Some(index), Some(code)) = (self.active_profile(), self.selected_key()) {
            let code_string = code.to_string();
            // Profile selectors can emit while their list is being replaced.
            if let Ok(profiles) = self.draft.profiles.try_borrow() {
                let action = profiles
                    .key_actions(&index)
                    .and_then(|mut actions| actions.find(|action| action.key_code == code_string));
                let friendly = caption(&KeyMap::default(), code);
                mapped = action.is_some();
                message = if let Some(action) = action {
                    format!(
                        "{friendly} · {} · {}",
                        actions::operation_name(action.operation_type),
                        action.value
                    )
                } else {
                    format!("{friendly} · Default assignment")
                };
            }
        }
        self.selected.set_label(&message);
        self.edit.set_sensitive(message != normal);
        self.remove.set_sensitive(mapped);
    }

    fn name_dialog(self: &Rc<Self>, create: bool) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        let Some(index) = self.active_profile() else {
            return;
        };
        let title = if create {
            "New profile"
        } else {
            "Rename profile"
        };
        let dialog = gtk::Dialog::builder()
            .title(title)
            .transient_for(&window)
            .modal(true)
            .build();
        dialog.add_button("Cancel", gtk::ResponseType::Cancel);
        dialog.add_button("Save", gtk::ResponseType::Accept);
        dialog.set_default_response(gtk::ResponseType::Accept);
        let entry = gtk::Entry::builder()
            .max_length(80)
            .activates_default(true)
            .margin_top(16)
            .margin_bottom(16)
            .margin_start(16)
            .margin_end(16)
            .build();
        entry.set_placeholder_text(Some("Profile name"));
        if !create {
            let profiles = self.draft.profiles.borrow();
            if let Some(profile) = profiles.profile(&index) {
                entry.set_text(profile.name);
            }
        }
        dialog.content_area().append(&entry);
        let weak = Rc::downgrade(self);
        dialog.connect_response(move |dialog, response| {
            if response == gtk::ResponseType::Accept {
                let value = entry.text();
                let value = value.trim();
                if value.is_empty() {
                    entry.grab_focus();
                    return;
                }
                if let Some(editor) = weak.upgrade() {
                    let mut document = editor.draft.profiles.borrow_mut();
                    let profiles = Arc::make_mut(&mut document);
                    if create {
                        if let Some(next) = profiles
                            .list_profiles()
                            .iter()
                            .filter_map(|item| item.index.parse::<i32>().ok())
                            .max()
                            .unwrap_or(0)
                            .checked_add(1)
                        {
                            profiles.append_profile(&next.to_string(), value, false);
                        }
                    } else {
                        let association = profiles
                            .profile(&index)
                            .map(|info| info.application.to_owned())
                            .unwrap_or_default();
                        let class = profiles.profile_x11_class(&index).map(str::to_owned);
                        profiles.rename_profile(&index, value);
                        profiles.set_profile_application(&index, &association, class.as_deref());
                    }
                    drop(document);
                    editor.refresh_choices();
                    editor.update_selection();
                }
            }
            dialog.close();
        });
        dialog.present();
    }

    fn file_dialog(self: &Rc<Self>, export: bool) {
        let kind = if export {
            gtk::FileChooserAction::Save
        } else {
            gtk::FileChooserAction::Open
        };
        let weak = Rc::downgrade(self);
        pick_file(
            &self.window,
            if export {
                "Export profile workspace"
            } else {
                "Import profile XML"
            },
            kind,
            move |path| {
                let Some(editor) = weak.upgrade() else { return };
                let runner = editor.tasks.clone();
                let magnetic = editor.layout.magnetic();
                if export {
                    let profiles = editor.draft.profiles.borrow().clone();
                    let advanced = magnetic.then(|| editor.draft.advanced.borrow().clone());
                    let done = runner.clone();
                    runner.start(
                        "Exporting profiles…",
                        false,
                        move |_, _| {
                            let xml = if let Some(definitions) = advanced {
                                Arc::unwrap_or_clone(profiles)
                                    .with_advanced_definitions(&definitions)
                                    .to_xml()
                            } else {
                                profiles.to_xml()
                            }
                            .map_err(io::Error::other)?;
                            write_profile(&path, &xml)
                        },
                        move |result| match result {
                            Ok(()) => done.message("Profile workspace exported", false),
                            Err(error) => done.error(error),
                        },
                    );
                } else {
                    let weak = Rc::downgrade(&editor);
                    let previous = editor.draft.profiles.borrow().clone();
                    runner.start(
                        "Importing profile XML…",
                        false,
                        move |_, _| {
                            let source = read_profile(&path)?;
                            let definitions = if magnetic {
                                source.advanced_definitions()
                            } else {
                                None
                            };
                            if definitions.is_some() {
                                // Keep definition identifiers paired with their source profiles.
                                Ok((source, definitions, Vec::new()))
                            } else {
                                let mut merged = Arc::unwrap_or_clone(previous);
                                let entries = source.list_profiles();
                                if entries.is_empty() {
                                    return Err(io::Error::new(
                                        io::ErrorKind::InvalidData,
                                        "The file has no profiles for this keyboard",
                                    ));
                                }
                                if merged.list_profiles().len() + entries.len() > 200 {
                                    return Err(io::Error::new(
                                        io::ErrorKind::InvalidData,
                                        "A workspace can hold up to 200 profiles",
                                    ));
                                }
                                let mut ids = std::collections::HashMap::new();
                        let mut imported = Vec::with_capacity(entries.len());
                        for item in entries {
                            let id = merged.import_profile(&source, item.index).ok_or_else(|| {
                                io::Error::new(io::ErrorKind::InvalidData, "Cannot allocate another profile index")
                            })?.to_string();
                            ids.insert(item.index.to_owned(), id.clone());
                            imported.push(id);
                        }
                        for id in &imported {
                            if !merged.remap_switch_targets(id, &ids) {
                                return Err(io::Error::new(io::ErrorKind::InvalidData,
                                    "A profile switch refers to a missing profile. Import its workspace to keep the switch."));
                            }
                        }
                        Ok((merged, None, imported))
                            }
                        },
                        move |result| match (weak.upgrade(), result) {
                            (Some(editor), Ok((profiles, definitions, imported))) => {
                                editor.accept_import(profiles, definitions, imported)
                            }
                            (Some(editor), Err(error)) => editor.tasks.error(error),
                            (None, _) => (),
                        },
                    );
                }
            },
        );
    }

    fn accept_import(
        self: &Rc<Self>,
        profiles: Profiles,
        definitions: Option<Profiles>,
        added: Vec<String>,
    ) {
        if let Some(definitions) = definitions {
            let weak = Rc::downgrade(self);
            let separate = profiles.list_profiles().is_empty();
            confirm(
                &self.tasks,
                &self.window,
                "Open advanced-key workspace?",
                if separate {
                    "This replaces the advanced-key definitions in your local workspace."
                } else {
                    "This replaces the local profiles and advanced keys with the ones in the file."
                },
                "Open workspace",
                move || {
                    let Some(editor) = weak.upgrade() else { return };
                    *editor.draft.advanced.borrow_mut() = Arc::new(definitions);
                    if !separate {
                        let choices = profiles.list_profiles();
                        let main = choices.first().map(|item| item.index.to_owned());
                        let function = choices
                            .get(1)
                            .map(|item| item.index.to_owned())
                            .or_else(|| main.clone());
                        *editor.draft.profiles.borrow_mut() = Arc::new(profiles);
                        editor.update_choices(main.as_deref(), function.as_deref());
                    }
                    editor.update_selection();
                    editor.tasks.message("Advanced-key workspace loaded", false);
                },
            );
            return;
        }
        *self.draft.profiles.borrow_mut() = Arc::new(profiles);
        let function = self.function.active_id();
        self.update_choices(
            added.first().map(String::as_str),
            added.get(1).map(String::as_str).or(function.as_deref()),
        );
        self.update_selection();
        self.tasks.message("Profiles imported", false);
    }

    fn validate(self: &Rc<Self>, status: gtk::Label, upload: bool) {
        let (Some(primary), Some(function)) = (self.main.active_id(), self.function.active_id())
        else {
            return;
        };
        let (primary, function) = (primary.to_string(), function.to_string());
        let profiles = self.draft.profiles.borrow().clone();
        let advanced = self.draft.advanced.borrow().clone();
        let layout = self.layout;
        let runner = self.tasks.clone();
        let done = runner.clone();
        let weak = Rc::downgrade(self);
        let requested = primary.clone();
        runner.start(
            "Checking your key assignments…",
            false,
            move |_, _| {
                encode_profile(
                    (&profiles, &requested),
                    (&profiles, &function),
                    Some(&advanced),
                    layout,
                )
            },
            move |result| match result {
                Err(error) => done.error(error),
                Ok(plan) if upload => {
                    let Some(editor) = weak.upgrade() else { return };
                    let extra = if plan.has_desktop_bindings() {
                        " Desktop actions are handled while this app stays open."
                    } else {
                        ""
                    };
                    let detail = format!(
                        "This replaces the saved keys, shortcuts, and macros in both layers.{extra}"
                    );
                    confirm(
                        &editor.tasks,
                        &editor.window,
                        "Replace the saved key assignments?",
                        &detail,
                        "Write to keyboard",
                        move || {
                            if weak.upgrade().is_none() {
                                return;
                            }
                            let result_tasks = done.clone();
                            done.start(
                                "Saving key assignments…",
                                false,
                                move |device, _| device.upload(&plan),
                                move |result| match (weak.upgrade(), result) {
                                    (Some(editor), Ok(result)) => {
                                        *editor.active_main.borrow_mut() = Some(primary);
                                        editor.upload_status(result);
                                    }
                                    (_, Err(error)) => result_tasks.error(error),
                                    _ => (),
                                },
                            );
                        },
                    );
                }
                Ok(_) => {
                    status.set_label("These key assignments are ready to save.");
                    done.message("", false);
                }
            },
        );
    }

    fn save_assignment(
        &self,
        index: &str,
        code: i16,
        kind: &str,
        value: &str,
        mode: &str,
        repeats: &str,
    ) {
        let code_str = code.to_string();
        let key_name = caption(&KeyMap::default(), code);
        let mut document = self.draft.profiles.borrow_mut();
        let profiles = Arc::make_mut(&mut document);
        if profiles.key_actions(index).is_none() {
            self.tasks
                .message("This profile has no key assignment section", true);
            return;
        }
        while profiles.remove_key_action(index, &code_str) {}
        profiles.append_key_action(
            index,
            KeyAction {
                key_code: &code_str,
                key_character: &key_name,
                operation_type: kind,
                repeat_count: repeats,
                mode,
                value,
            },
        );
        drop(document);
        self.update_selection();
        self.tasks
            .message("Assignment staged in this profile", false);
    }
}

fn read_profile(path: &Path) -> io::Result<Profiles> {
    const LIMIT: u64 = 16 * 1024 * 1024;
    let mut file = File::options()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Choose a regular XML file",
        ));
    }
    let mut bytes = Vec::new();
    (&mut file).take(LIMIT + 1).read_to_end(&mut bytes)?;
    Profiles::from_xml(&bytes)
}

fn write_profile(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let directory = path.parent().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Choose a destination directory",
        )
    })?;
    let temp = directory.join(format!(".dragon-driver-export-{}.tmp", std::process::id()));
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&temp)?;
    let result = file.write_all(bytes).and_then(|()| fs::rename(&temp, path));
    if result.is_err() {
        let _ = fs::remove_file(&temp);
    }
    result
}
