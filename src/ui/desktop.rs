mod x11;

#[cfg(test)]
pub(super) use x11::test_focus as x11_test_focus;

use std::io;
use std::rc::Rc;
use std::time::{Duration, Instant};

use dragon_driver::keyboard::desktop::DesktopBinding;
use gtk::{gio, glib, prelude::*};

use super::worker::Tasks;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Focus {
    pub window: u32,
    pub app_id: Option<String>,
    pub classes: Vec<String>,
}

pub(super) enum Event {
    Binding(DesktopBinding, Option<u64>),
    Focus(Option<Focus>),
    Message(String),
}

pub(super) struct Desktop {
    native_x11: bool,
    focus: Option<x11::X11>,
    paste: Option<x11::Paste>,
    previous: Option<Focus>,
    next_poll: Instant,
}

impl Desktop {
    pub fn new(native_x11: bool) -> Self {
        Self {
            native_x11,
            focus: None,
            paste: None,
            previous: None,
            next_poll: Instant::now(),
        }
    }

    pub fn watch(&mut self, enabled: bool) -> io::Result<()> {
        self.focus = if enabled && self.native_x11 {
            Some(x11::X11::open()?)
        } else if enabled {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "This desktop does not expose the foreground window",
            ));
        } else {
            None
        };
        self.previous = None;
        self.next_poll = Instant::now();
        Ok(())
    }

    pub fn watching(&self) -> bool {
        self.focus.is_some()
    }

    pub fn poll(&mut self, now: Instant) -> Option<Event> {
        let focus = self.focus.as_ref()?;
        if now < self.next_poll {
            return None;
        }
        self.next_poll = now + Duration::from_millis(250);
        let current = focus.focus();
        if current != self.previous {
            self.previous = current.clone();
            Some(Event::Focus(current))
        } else {
            None
        }
    }

    pub fn still_focused(&self, target: u32) -> bool {
        self.focus
            .as_ref()
            .and_then(x11::X11::focus)
            .is_some_and(|focus| focus.window == target)
    }

    pub fn paste_target(&mut self) -> Option<u64> {
        if !self.native_x11 {
            return None;
        }
        if self.paste.is_none() {
            self.paste = x11::Paste::open().ok();
        }
        self.paste.as_ref()?.focus()
    }

    pub fn paste(&self, target: u64) -> io::Result<()> {
        self.paste
            .as_ref()
            .ok_or_else(|| io::Error::other("The text is ready on the clipboard"))?
            .paste(target)
    }
}

pub(super) fn native_x11() -> bool {
    gtk::gdk::Display::default().is_some_and(|display| display.type_().name() == "GdkX11Display")
        && std::env::var_os("XDG_SESSION_TYPE").is_none_or(|kind| kind != "wayland")
}

#[derive(Clone)]
pub(super) struct Application {
    pub id: String,
    pub name: String,
    pub class: Option<String>,
}

pub(super) fn choose_application(
    tasks: &Rc<Tasks>,
    window: &glib::WeakRef<gtk::Window>,
    choose: impl FnOnce(Application) + 'static,
) {
    let window = window.clone();
    let tasks_done = tasks.clone();
    tasks.start(
        "Finding installed applications…",
        false,
        |_, _| {
            let mut apps = gio::AppInfo::all()
                .into_iter()
                .filter_map(|app| {
                    if !app.should_show() {
                        return None;
                    }
                    let id = app.id()?.to_string();
                    let name = app.display_name().to_string();
                    let class = app
                        .downcast::<gio::DesktopAppInfo>()
                        .ok()
                        .and_then(|app| app.startup_wm_class())
                        .map(|class| class.to_string());
                    valid_id(&id).then_some(Application { id, name, class })
                })
                .collect::<Vec<_>>();
            apps.sort_by_cached_key(|item| item.name.to_lowercase());
            Ok(apps)
        },
        move |result| match result {
            Ok(applications) => {
                let Some(window) = window.upgrade() else {
                    return;
                };
                let dialog = gtk::Dialog::builder()
                    .title("Choose an installed application")
                    .transient_for(&window)
                    .modal(true)
                    .default_width(460)
                    .build();
                dialog.add_button("Cancel", gtk::ResponseType::Cancel);
                let select = dialog.add_button("Choose application", gtk::ResponseType::Accept);
                select.set_sensitive(!applications.is_empty());
                let list = gtk::ComboBoxText::new();
                for app in &applications {
                    list.append_text(&app.name);
                }
                list.set_active(Some(0));
                for margin in [
                    gtk::Widget::set_margin_top,
                    gtk::Widget::set_margin_bottom,
                    gtk::Widget::set_margin_start,
                    gtk::Widget::set_margin_end,
                ] {
                    margin(list.upcast_ref(), 16);
                }
                dialog.content_area().append(&list);
                let choose = std::cell::RefCell::new(Some(choose));
                dialog.connect_response(move |dialog, response| {
                    dialog.close();
                    if response == gtk::ResponseType::Accept
                        && let Some(item) = list
                            .active()
                            .and_then(|index| applications.get(index as usize))
                        && let Some(choose) = choose.borrow_mut().take()
                    {
                        choose(item.clone());
                    }
                });
                dialog.present();
                tasks_done.message("", false);
            }
            Err(error) => tasks_done.error(error),
        },
    );
}

pub(super) fn valid_id(id: &str) -> bool {
    id.ends_with(".desktop")
        && id.len() <= 255
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
}

pub(super) fn launch(id: &str) -> io::Result<()> {
    if !valid_id(id) {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Choose an installed Linux app for this binding",
        ));
    }
    let app = gio::DesktopAppInfo::new(id).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "The application is no longer installed",
        )
    })?;
    app.launch(&[], gio::AppLaunchContext::NONE)
        .map_err(io::Error::other)
}

pub(super) fn match_strength(focus: &Focus, id: &str, class: Option<&str>) -> u8 {
    if !valid_id(id) {
        return 0;
    }
    let app = id.strip_suffix(".desktop").unwrap_or(id);
    if focus
        .app_id
        .as_deref()
        .is_some_and(|value| value == app || value == id)
    {
        return 3;
    }
    if let Some(class) = class.filter(|value| !value.is_empty())
        && focus
            .classes
            .iter()
            .any(|value| value.eq_ignore_ascii_case(class))
    {
        return 2;
    }
    u8::from(
        focus
            .classes
            .iter()
            .any(|value| value.eq_ignore_ascii_case(app)),
    )
}

#[derive(Default)]
pub(super) struct Follow {
    candidate: Option<(String, Instant)>,
    previous_request: Option<Instant>,
}

impl Follow {
    pub fn ready(
        &mut self,
        id: Option<&str>,
        active: Option<&str>,
        now: Instant,
    ) -> Option<String> {
        let Some(id) = id.filter(|id| Some(*id) != active) else {
            self.candidate = None;
            return None;
        };
        let Some((candidate, since)) = self
            .candidate
            .as_ref()
            .filter(|(candidate, _)| candidate == id)
        else {
            self.candidate = Some((id.into(), now));
            return None;
        };
        if now.duration_since(*since) < Duration::from_millis(450)
            || self
                .previous_request
                .is_some_and(|time| now.duration_since(time) < Duration::from_secs(3))
        {
            return None;
        }
        self.previous_request = Some(now);
        Some(candidate.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desktop_manifest_ids_cannot_be_commands_or_paths() {
        assert!(valid_id("org.mozilla.Firefox.desktop"));
        for value in [
            "/bin/sh",
            "a.desktop;ls",
            "../a.desktop",
            "a.desktop --help",
            "a.desktop\0",
        ] {
            assert!(!valid_id(value));
        }
        let focus = Focus {
            window: 1,
            app_id: Some("org.mozilla.Firefox".into()),
            classes: vec!["firefox".into()],
        };
        assert_eq!(
            match_strength(&focus, "org.mozilla.Firefox.desktop", None),
            3
        );
        assert_eq!(
            match_strength(&focus, "some.Other.desktop", Some("FIREFOX")),
            2
        );
        assert_eq!(match_strength(&focus, "firefox.desktop", None), 1);
        assert_eq!(match_strength(&focus, "myfirefox.desktop", None), 0);
    }

    #[test]
    fn foreground_transitions_require_dwell_and_avoid_repeat_uploads() {
        let now = Instant::now();
        let mut follow = Follow::default();
        assert_eq!(follow.ready(Some("1"), None, now), None);
        assert_eq!(
            follow.ready(Some("2"), None, now + Duration::from_millis(200)),
            None
        );
        assert_eq!(
            follow.ready(Some("2"), None, now + Duration::from_millis(700)),
            Some("2".into())
        );
        assert_eq!(
            follow.ready(Some("2"), Some("2"), now + Duration::from_secs(1)),
            None
        );
        assert_eq!(
            follow.ready(Some("1"), Some("2"), now + Duration::from_secs(2)),
            None
        );
        assert_eq!(
            follow.ready(Some("1"), Some("2"), now + Duration::from_secs(3)),
            None
        );
        assert_eq!(
            follow.ready(Some("1"), Some("2"), now + Duration::from_secs(4)),
            Some("1".into())
        );
    }
}
