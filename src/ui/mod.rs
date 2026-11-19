mod access;
mod color_wheel;
mod controls;
mod desktop;
mod desktop_theme;
mod keys;
mod lighting_trace;
mod profiles;
mod readbacks;
mod responsive;
mod worker;

#[cfg(test)]
mod smoke;

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use gtk::{Align, Orientation, gio, glib, prelude::*};
use worker::{Candidate, Connected, Tasks};

struct View {
    window: glib::WeakRef<gtk::ApplicationWindow>,
    tasks: Rc<Tasks>,
    devices: gtk::ComboBoxText,
    pages: gtk::Stack,
    rail: gtk::StackSidebar,
    keybed: gtk::Box,
    readbacks: RefCell<Option<Rc<readbacks::Readbacks>>>,
    columns: gtk::Adjustment,
    screens: gtk::Stack,
    title: gtk::Label,
    details: gtk::Label,
    empty_detail: gtk::Label,
    grant: gtk::Button,
    denied: Cell<Option<usize>>,
    prompted: Cell<bool>,
    authorizing: Cell<bool>,
    candidates: RefCell<Vec<Candidate>>,
    connected: RefCell<Option<Connected>>,
    updating: Cell<bool>,
    drafts: RefCell<HashMap<&'static str, Rc<profiles::Draft>>>,
}

pub(super) fn install_style(app: &gtk::Application) {
    if let Some(display) = gtk::gdk::Display::default() {
        let provider = gtk::CssProvider::new();
        provider.load_from_data(include_str!("theme.css"));
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    }
    desktop_theme::follow(app);
}

pub(super) fn open(app: &gtk::Application) {
    lighting_trace::event(format_args!("app opened"));
    if let Some(view) = build_window(app) {
        view.scan();
    }
}

fn build_window(app: &gtk::Application) -> Option<Rc<View>> {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title(crate::APP_NAME)
        .icon_name(crate::APP_ID)
        .default_width(1060)
        .default_height(750)
        .build();
    window.add_css_class("dragon-driver");
    track_control_borders(&window);

    let header = gtk::HeaderBar::new();
    let brand = gtk::Box::new(Orientation::Horizontal, 9);
    let logo = glib::Bytes::from_static(include_bytes!(
        "../../packaging/org.dragondriver.DragonDriver.png"
    ));
    let icon = gtk::Image::from_gicon(&gio::BytesIcon::new(&logo));
    icon.set_pixel_size(24);
    icon.add_css_class("brand-icon");
    brand.append(&icon);
    brand.append(&label(crate::APP_NAME, "brand"));
    header.set_title_widget(Some(&brand));
    let connection = gtk::Box::new(Orientation::Horizontal, 8);
    let devices = gtk::ComboBoxText::new();
    devices.set_tooltip_text(Some("Select keyboard"));
    devices.set_visible(false);
    connection.append(&devices);
    header.pack_start(&connection);
    window.set_titlebar(Some(&header));

    let screens = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .transition_duration(160)
        .vexpand(true)
        .build();
    let (empty, empty_detail, empty_scan, grant) = empty_screen();
    screens.add_named(&empty, Some("empty"));

    let pages = gtk::Stack::builder()
        .transition_type(gtk::StackTransitionType::Crossfade)
        .transition_duration(160)
        .hexpand(true)
        .vexpand(true)
        .hhomogeneous(true)
        .vhomogeneous(true)
        .build();
    let rail = gtk::StackSidebar::new();
    rail.set_stack(&pages);
    rail.set_width_request(128);
    let keybed = gtk::Box::new(Orientation::Vertical, 0);
    keybed.set_margin_start(24);
    keybed.set_margin_end(24);
    keybed.set_margin_top(12);
    let content = gtk::Box::new(Orientation::Vertical, 0);
    content.append(&keybed);
    content.append(&pages);
    let holder = keybed.clone();
    pages.connect_visible_child_name_notify(move |pages| {
        let page = pages.visible_child_name();
        lighting_trace::event(format_args!("page {page:?}"));
        holder.set_visible(page.as_deref() != Some("device"));
        if let Some(area) = holder.first_child() {
            if page.as_deref() == Some("lighting") {
                area.add_css_class("lighting-preview");
            } else {
                area.remove_css_class("lighting-preview");
            }
        }
    });
    let device_screen = gtk::Box::new(Orientation::Horizontal, 0);
    device_screen.append(&rail);
    device_screen.append(&content);

    let title = label("", "brand");
    let details = label("", "muted");
    details.set_hexpand(true);
    details.set_xalign(1.0);
    details.set_ellipsize(gtk::pango::EllipsizeMode::End);
    let body = gtk::Box::new(Orientation::Vertical, 0);
    let top = gtk::Box::new(Orientation::Horizontal, 13);
    top.add_css_class("device-bar");
    top.append(&gtk::Image::from_icon_name("input-keyboard-symbolic"));
    top.append(&title);
    top.append(&details);
    body.append(&top);
    body.append(&gtk::Separator::new(Orientation::Horizontal));
    body.append(&device_screen);
    screens.add_named(&body, Some("keyboard"));

    let status = label("", "muted");
    status.set_wrap(true);
    status.set_height_request(20);
    status.set_hexpand(true);
    status.set_accessible_role(gtk::AccessibleRole::Status);
    let spinner = gtk::Spinner::new();
    spinner.set_visible(false);
    let stop = gtk::Button::with_label("Stop");
    stop.set_visible(false);
    let copy_text = gtk::Button::with_label("Copy text");
    copy_text.set_visible(false);
    let bottom = gtk::Box::new(Orientation::Horizontal, 10);
    bottom.add_css_class("status");
    bottom.append(&spinner);
    bottom.append(&status);
    bottom.append(&copy_text);
    bottom.append(&stop);
    let root = gtk::Box::new(Orientation::Vertical, 0);
    root.append(&screens);
    root.append(&gtk::Separator::new(Orientation::Horizontal));
    root.append(&bottom);
    window.set_child(Some(&root));

    // The navigation and device choice are gated together while a command owns the session.
    // The window and the separate Stop button still receive input.
    let controls = gtk::Box::new(Orientation::Vertical, 0);
    let tasks = match Tasks::new(&status, &spinner, &controls, &stop, &copy_text) {
        Ok(tasks) => tasks,
        Err(error) => {
            status.set_label(&format!("Could not start the keyboard app. {error}"));
            window.present();
            return None;
        }
    };
    // The header and the work area live in separate GTK containers.
    controls
        .bind_property("sensitive", &screens, "sensitive")
        .sync_create()
        .build();
    controls
        .bind_property("sensitive", &connection, "sensitive")
        .sync_create()
        .build();
    let columns = responsive::monitor_columns(&window);
    let view = Rc::new(View {
        window: window.downgrade(),
        tasks,
        devices,
        pages,
        rail,
        keybed,
        readbacks: RefCell::new(None),
        columns,
        screens,
        title,
        details,
        empty_detail,
        grant,
        denied: Cell::new(None),
        prompted: Cell::new(false),
        authorizing: Cell::new(false),
        candidates: RefCell::new(Vec::new()),
        connected: RefCell::new(None),
        updating: Cell::new(false),
        drafts: RefCell::new(HashMap::new()),
    });
    let weak = Rc::downgrade(&view);
    empty_scan.connect_clicked(move |_| {
        if let Some(view) = weak.upgrade() {
            view.scan();
        }
    });
    let weak = Rc::downgrade(&view);
    view.grant.connect_clicked(move |_| {
        if let Some(view) = weak.upgrade() {
            view.authorize();
        }
    });
    let weak = Rc::downgrade(&view);
    view.devices.connect_changed(move |select| {
        if let Some(view) = weak.upgrade()
            && !view.updating.get()
        {
            view.connect(selected_index(select));
        }
    });
    let cancel = view.tasks.cancel.clone();
    stop.connect_clicked(move |stop| {
        cancel.store(true, Ordering::Relaxed);
        stop.set_sensitive(false);
    });
    let weak = Rc::downgrade(&view.tasks);
    view.pages.connect_visible_child_name_notify(move |pages| {
        if pages.visible_child_name().as_deref() != Some("actuation")
            && let Some(tasks) = weak.upgrade()
            && tasks.tracking.get()
        {
            tasks.cancel.store(true, Ordering::Relaxed);
        }
    });
    let weak = Rc::downgrade(&view);
    window.connect_is_active_notify(move |window| {
        if window.is_active()
            && let Some(view) = weak.upgrade()
            && let Some(readbacks) = view.readbacks.borrow().as_ref()
        {
            readbacks.refresh();
        }
    });
    let weak = Rc::downgrade(&view);
    glib::timeout_add_local(Duration::from_secs(1), move || {
        let Some(view) = weak.upgrade() else {
            return glib::ControlFlow::Break;
        };
        if let Some(window) = view.window.upgrade()
            && let Some(readbacks) = view.readbacks.borrow().as_ref()
            && let Some(page) = view.pages.visible_child_name()
        {
            readbacks.poll_visible(if window.is_active() {
                page.as_str()
            } else {
                ""
            });
        }
        glib::ControlFlow::Continue
    });
    let monitor = view.watch_devices();
    let sandbox_watch = view.watch_sandbox_devices();
    let owned = view.clone();
    let closing = Cell::new(false);
    window.connect_close_request(move |window| {
        owned.tasks.cancel.store(true, Ordering::Relaxed);
        owned.readbacks.borrow_mut().take();
        if let Some(sandbox_watch) = &sandbox_watch {
            sandbox_watch.set(false);
        }
        if let Some(monitor) = &monitor {
            monitor.cancel();
        }
        if !owned.tasks.busy.get() {
            return glib::Propagation::Proceed;
        }
        if !closing.replace(true) {
            let window = window.downgrade();
            let tasks = Rc::downgrade(&owned.tasks);
            glib::timeout_add_local(Duration::from_millis(25), move || {
                if let (Some(window), Some(tasks)) = (window.upgrade(), tasks.upgrade()) {
                    if tasks.busy.get() {
                        return glib::ControlFlow::Continue;
                    }
                    window.close();
                }
                glib::ControlFlow::Break
            });
        }
        glib::Propagation::Stop
    });
    window.present();
    Some(view)
}

fn track_control_borders(window: &gtk::ApplicationWindow) {
    fn update(window: &gtk::ApplicationWindow) {
        let text = window.style_context().color();
        let light_text = 0.2126 * text.red() + 0.7152 * text.green() + 0.0722 * text.blue() > 0.5;
        if light_text {
            window.add_css_class("light-control-borders");
        } else {
            window.remove_css_class("light-control-borders");
        }
    }

    window.connect_map(update);
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    let mut signals = Vec::new();
    for name in [
        "gtk-theme-name",
        "gtk-application-prefer-dark-theme",
        "gtk-interface-color-scheme",
    ] {
        if settings.find_property(name).is_some() {
            let window = window.downgrade();
            signals.push(settings.connect_notify_local(Some(name), move |_, _| {
                let window = window.clone();
                glib::idle_add_local_once(move || {
                    if let Some(window) = window.upgrade() {
                        update(&window);
                    }
                });
            }));
        }
    }
    let signals = RefCell::new(signals);
    window.connect_destroy(move |_| {
        for signal in signals.take() {
            settings.disconnect(signal);
        }
    });
}

fn selected_index(select: &gtk::ComboBoxText) -> Option<usize> {
    select.active().map(|index| index as usize)
}

impl View {
    fn scan(self: &Rc<Self>) {
        if self.tasks.busy.get() || self.authorizing.get() {
            return;
        }
        self.denied.set(None);
        self.grant.set_visible(false);
        self.tasks.clear_pending_text();
        self.tasks
            .revision
            .set(self.tasks.revision.get().wrapping_add(1));
        self.screens.set_visible_child_name("empty");
        self.connected.borrow_mut().take();
        self.readbacks.borrow_mut().take();
        self.empty_detail
            .set_label("Checking for a compatible USB keyboard…");
        let weak = Rc::downgrade(self);
        self.tasks.start(
            "Scanning for keyboards…",
            false,
            |device, _| device.scan(),
            move |result| {
                let Some(view) = weak.upgrade() else { return };
                match result {
                    Ok(candidates) => {
                        view.updating.set(true);
                        view.devices.remove_all();
                        for candidate in &candidates {
                            view.devices.append_text(&candidate.name);
                        }
                        *view.candidates.borrow_mut() = candidates;
                        let found = view.candidates.borrow().len();
                        view.devices.set_visible(found > 1);
                        view.devices.set_active((found == 1).then_some(0));
                        view.updating.set(false);
                        view.tasks.message("", false);
                        view.empty_detail.set_label(if found > 1 {
                            "Choose a keyboard in the title bar."
                        } else {
                            "Connect a compatible Redragon keyboard over USB."
                        });
                        if found == 1 {
                            view.connect(Some(0));
                        }
                    }
                    Err(error) => {
                        if error.kind() == std::io::ErrorKind::PermissionDenied && access::can_request() {
                            view.empty_detail.set_label("Set up keyboard access once to get started.");
                            view.grant.set_visible(true);
                            if !view.prompted.replace(true) {
                                view.authorize();
                            }
                        } else {
                            view.empty_detail.set_label("The keyboard scan failed. Set up the host companion and try again.");
                            view.tasks.error(error);
                        }
                    }
                }
            },
        );
    }

    fn connect(self: &Rc<Self>, index: Option<usize>) {
        if self.tasks.busy.get() || self.authorizing.get() {
            return;
        }
        self.denied.set(None);
        self.grant.set_visible(false);
        let Some(index) = index else { return };
        let Some(candidate) = self.candidates.borrow().get(index).cloned() else {
            return;
        };
        let can_authorize = access::can_request();
        self.tasks.clear_pending_text();
        self.tasks
            .revision
            .set(self.tasks.revision.get().wrapping_add(1));
        self.connected.borrow_mut().take();
        self.readbacks.borrow_mut().take();
        self.screens.set_visible_child_name("empty");
        self.empty_detail.set_label("Opening your keyboard…");
        let weak = Rc::downgrade(self);
        let revision = self.tasks.revision.get();
        self.tasks.start(
            "Opening keyboard…",
            false,
            move |device, _| device.connect(candidate, revision),
            move |result| {
                let Some(view) = weak.upgrade() else { return };
                match result {
                    Ok(mut device) => {
                        let palette_error = device.palette_error.take();
                        view.show_keyboard(device);
                        if let Some(error) = palette_error {
                            view.tasks.message(&error, true);
                        }
                        if let Some(readbacks) = view.readbacks.borrow().as_ref() {
                            readbacks.refresh();
                        }
                    }
                    Err(error) => {
                        let denied = error.kind() == std::io::ErrorKind::PermissionDenied;
                        view.empty_detail.set_label(if denied && can_authorize {
                            "Allow access to this keyboard to load its settings."
                        } else if denied {
                            "See the project README to give your desktop access to this keyboard."
                        } else {
                            "This keyboard could not be opened. Check the status below."
                        });
                        view.tasks.error(error);
                        if denied && can_authorize {
                            view.denied.set(Some(index));
                            view.grant.set_visible(true);
                            if !view.prompted.replace(true) {
                                view.authorize();
                            }
                        }
                    }
                }
            },
        );
    }

    fn authorize(self: &Rc<Self>) {
        if !access::can_request() {
            return;
        }
        let index = self.denied.get();
        if self.authorizing.replace(true) {
            return;
        }
        self.grant.set_sensitive(false);
        self.tasks.message("Waiting for your approval…", false);
        let weak = Rc::downgrade(self);
        let complete = move |result: Result<(), String>| {
            let Some(view) = weak.upgrade() else { return };
            view.authorizing.set(false);
            view.grant.set_sensitive(true);
            match result {
                Ok(()) => match index {
                    Some(index) => view.connect(Some(index)),
                    None => view.scan(),
                },
                Err(error) => view.tasks.message(&error, true),
            }
        };
        if let Err(error) = access::request(complete) {
            self.authorizing.set(false);
            self.grant.set_sensitive(true);
            self.tasks.error(error);
        }
    }

    fn show_keyboard(&self, device: Connected) {
        let Some(window) = self.window.upgrade() else {
            return;
        };
        self.rail.set_stack(&gtk::Stack::new());
        while let Some(child) = self.pages.first_child() {
            self.pages.remove(&child);
        }
        while let Some(child) = self.keybed.first_child() {
            self.keybed.remove(&child);
        }
        let grid = keys::KeyGrid::new(device.layout);
        self.keybed.append(&grid.area);
        let readbacks = readbacks::Readbacks::new(&device, &grid, &self.tasks);
        self.title.set_label(&device.name);
        self.details
            .set_label(&format!("Firmware {}", device.firmware));
        self.tasks.message("Connected", false);
        let start = if device.layout.magnetic() {
            add_page(
                &self.pages,
                "actuation",
                "Actuation",
                &controls::actuation(&grid, &self.tasks, &readbacks),
            );
            "actuation"
        } else {
            "lighting"
        };
        add_page(
            &self.pages,
            "lighting",
            "Lighting",
            &controls::lighting(&device, &grid, &self.tasks, &readbacks),
        );
        if device.layout.onboard_profiles {
            let draft = self
                .drafts
                .borrow_mut()
                .entry(device.layout.family)
                .or_insert_with(profiles::Draft::new)
                .clone();
            add_page(
                &self.pages,
                "profiles",
                "Profiles",
                &profiles::page(&device, &grid, &self.tasks, &window, draft),
            );
        }
        add_page(
            &self.pages,
            "device",
            "Device",
            &controls::device(&device, &self.tasks, &window, &readbacks, &self.columns),
        );
        self.pages.set_visible_child_name(start);
        self.rail.set_stack(&self.pages);
        self.screens.set_visible_child_name("keyboard");
        *self.connected.borrow_mut() = Some(device);
        *self.readbacks.borrow_mut() = Some(readbacks);
    }

    fn watch_sandbox_devices(self: &Rc<Self>) -> Option<Rc<Cell<bool>>> {
        // A Flatpak sees the system HID inventory but its private /dev does not
        // receive host hotplug events. Refresh only when an interface path changes.
        if !std::path::Path::new("/.flatpak-info").exists() {
            return None;
        }
        fn inventory() -> Vec<std::path::PathBuf> {
            let mut paths: Vec<_> = std::fs::read_dir("/sys/class/hidraw")
                .into_iter()
                .flatten()
                .filter_map(|entry| std::fs::canonicalize(entry.ok()?.path()).ok())
                .collect();
            paths.sort_unstable();
            paths
        }
        let keep = Rc::new(Cell::new(true));
        let active = keep.clone();
        let view = Rc::downgrade(self);
        let mut previous = inventory();
        glib::timeout_add_local(Duration::from_millis(750), move || {
            let Some(view) = view.upgrade().filter(|_| active.get()) else {
                return glib::ControlFlow::Break;
            };
            if !view.tasks.busy.get() {
                let current = inventory();
                if current != previous {
                    previous = current;
                    view.scan();
                }
            }
            glib::ControlFlow::Continue
        });
        Some(keep)
    }

    fn watch_devices(self: &Rc<Self>) -> Option<gio::FileMonitor> {
        let monitor = gio::File::for_path("/dev")
            .monitor_directory(gio::FileMonitorFlags::NONE, gio::Cancellable::NONE)
            .ok()?;
        let weak = Rc::downgrade(self);
        let queued = Rc::new(Cell::new(false));
        monitor.connect_changed(move |_, file, _, event| {
            use gio::FileMonitorEvent::{AttributeChanged, Created, Deleted, MovedIn, MovedOut};
            if !matches!(
                event,
                AttributeChanged | Created | Deleted | MovedIn | MovedOut
            ) {
                return;
            }
            let Some(path) = file.path() else { return };
            if !path
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("hidraw"))
            {
                return;
            }
            let Some(view) = weak.upgrade() else { return };
            if let Some(device) = view.connected.borrow().as_ref()
                && (!matches!(event, Deleted | MovedOut) || path != device.node)
            {
                return;
            }
            if matches!(event, Deleted | MovedOut) && view.tasks.tracking.get() {
                view.tasks.cancel.store(true, Ordering::Relaxed);
            }
            if queued.replace(true) {
                return;
            }
            let queued = queued.clone();
            let weak = Rc::downgrade(&view);
            glib::timeout_add_local(Duration::from_millis(280), move || {
                let Some(view) = weak.upgrade() else {
                    return glib::ControlFlow::Break;
                };
                if view.tasks.busy.get() || view.authorizing.get() {
                    return glib::ControlFlow::Continue;
                }
                queued.set(false);
                view.scan();
                glib::ControlFlow::Break
            });
        });
        Some(monitor)
    }
}

fn empty_screen() -> (gtk::Box, gtk::Label, gtk::Button, gtk::Button) {
    let layout = gtk::Box::builder()
        .orientation(Orientation::Vertical)
        .spacing(15)
        .halign(Align::Center)
        .valign(Align::Center)
        .margin_start(28)
        .margin_end(28)
        .build();
    let symbol = gtk::Image::from_icon_name("input-keyboard-symbolic");
    symbol.set_pixel_size(42);
    symbol.set_halign(Align::Center);
    symbol.add_css_class("empty-icon");
    layout.append(&symbol);
    let title = label("Connect your keyboard", "page-title");
    title.set_xalign(0.5);
    layout.append(&title);
    let explanation = label("Connect a compatible Redragon keyboard over USB.", "muted");
    explanation.set_wrap(true);
    explanation.set_justify(gtk::Justification::Center);
    explanation.set_xalign(0.5);
    explanation.set_max_width_chars(52);
    layout.append(&explanation);
    let button = primary("Scan for keyboards");
    button.set_halign(Align::Center);
    button.set_margin_top(5);
    layout.append(&button);
    let grant = primary("Allow keyboard access");
    grant.set_halign(Align::Center);
    grant.set_visible(false);
    layout.append(&grant);
    (layout, explanation, button, grant)
}

fn add_page(stack: &gtk::Stack, id: &str, tab: &str, content: &gtk::Box) {
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .child(content)
        .build();
    stack.add_titled(&scroll, Some(id), tab);
}

fn label(text: &str, class: &str) -> gtk::Label {
    let label = gtk::Label::new(Some(text));
    label.set_xalign(0.0);
    if !class.is_empty() {
        label.add_css_class(match class {
            "muted" => "dim-label",
            "page-title" => "title-1",
            "section-title" => "heading",
            class => class,
        });
    }
    label
}

fn primary(text: &str) -> gtk::Button {
    let button = gtk::Button::with_label(text);
    button.add_css_class("suggested-action");
    button
}

fn page(title: &str, icon: &str) -> gtk::Box {
    let content = gtk::Box::new(Orientation::Vertical, 14);
    content.set_margin_top(16);
    content.set_margin_bottom(24);
    content.set_margin_start(24);
    content.set_margin_end(24);
    let header = gtk::Box::new(Orientation::Horizontal, 10);
    header.add_css_class("page-header");
    let symbol = gtk::Image::from_icon_name(icon);
    symbol.add_css_class("dim-label");
    symbol.set_pixel_size(20);
    header.append(&symbol);
    let title = label(title, "page-title");
    title.set_accessible_role(gtk::AccessibleRole::Heading);
    header.append(&title);
    content.append(&header);
    content
}

fn panel(title: &str) -> gtk::Box {
    let panel = gtk::Box::new(Orientation::Vertical, 6);
    panel.add_css_class("panel");
    if !title.is_empty() {
        let heading = label(title, "section-title");
        heading.add_css_class("panel-heading");
        heading.set_accessible_role(gtk::AccessibleRole::Heading);
        panel.append(&heading);
        panel.append(&gtk::Separator::new(Orientation::Horizontal));
    }
    panel
}

fn field(caption: &str, widget: &impl IsA<gtk::Widget>) -> gtk::Box {
    let row = gtk::Box::new(Orientation::Horizontal, 12);
    row.add_css_class("settings-row");
    let caption = label(caption, "");
    caption.set_mnemonic_widget(Some(widget));
    caption.set_hexpand(true);
    caption.set_wrap(true);
    caption.set_valign(Align::Center);
    row.append(&caption);
    row.append(widget);
    row
}

fn confirm(
    tasks: &Rc<Tasks>,
    window: &glib::WeakRef<impl IsA<gtk::Window>>,
    title: &str,
    detail: &str,
    verb: &str,
    action: impl FnOnce() + 'static,
) {
    let Some(window) = window.upgrade() else {
        return;
    };
    let dialog = gtk::MessageDialog::builder()
        .transient_for(&window)
        .modal(true)
        .text(title)
        .secondary_text(detail)
        .build();
    dialog.add_button("Cancel", gtk::ResponseType::Cancel);
    let accept = dialog.add_button(verb, gtk::ResponseType::Accept);
    accept.add_css_class("destructive-action");
    dialog.set_default_response(gtk::ResponseType::Cancel);
    let action = RefCell::new(Some(action));
    let revision = tasks.revision.get();
    let tasks = Rc::downgrade(tasks);
    dialog.connect_response(move |dialog, result| {
        dialog.close();
        if result == gtk::ResponseType::Accept
            && let Some(action) = action.borrow_mut().take()
            && let Some(tasks) = tasks.upgrade()
        {
            if tasks.revision.get() == revision {
                action();
            } else {
                tasks.message(
                    "The keyboard connection changed. Open this action again.",
                    true,
                );
            }
        }
    });
    dialog.present();
}

fn pick_file(
    window: &glib::WeakRef<gtk::ApplicationWindow>,
    title: &str,
    kind: gtk::FileChooserAction,
    action: impl FnOnce(std::path::PathBuf) + 'static,
) {
    let Some(window) = window.upgrade() else {
        return;
    };
    let verb = if kind == gtk::FileChooserAction::Save {
        "Save"
    } else {
        "Open"
    };
    let chooser =
        gtk::FileChooserNative::new(Some(title), Some(&window), kind, Some(verb), Some("Cancel"));
    chooser.set_modal(true);
    if kind == gtk::FileChooserAction::Save {
        chooser.set_current_name("dragon-driver-profile.xml");
    }
    glib::MainContext::default().spawn_local(async move {
        let response = chooser.run_future().await;
        let path = chooser.file().and_then(|file| file.path());
        chooser.destroy();
        if response == gtk::ResponseType::Accept
            && let Some(path) = path
        {
            action(path);
        }
    });
}
