use std::collections::HashMap;
use std::ffi::OsStr;
use std::path::Path;

use gtk::{gio, glib, prelude::*};

const BUS: &str = "org.freedesktop.portal.Desktop";
const INTERFACE: &str = "org.freedesktop.portal.Settings";
const DESKTOP: &str = "org.gnome.desktop.interface";
const APPEARANCE: &str = "org.freedesktop.appearance";

type PortalSettings = HashMap<String, HashMap<String, glib::Variant>>;

pub(super) fn follow(app: &gtk::Application) {
    if std::env::var_os("GTK_THEME").is_some() {
        return;
    }
    let Some(settings) = gtk::Settings::default() else {
        return;
    };
    let app = app.downgrade();
    glib::MainContext::default().spawn_local(async move {
        let Ok(portal) = gio::DBusProxy::for_bus_future(
            gio::BusType::Session,
            gio::DBusProxyFlags::DO_NOT_LOAD_PROPERTIES,
            None,
            BUS,
            "/org/freedesktop/portal/desktop",
            INTERFACE,
        )
        .await
        else {
            return;
        };
        let effects = settings.clone();
        portal.connect_local("g-signal", false, move |values| {
            if values[2].get::<String>().as_deref() == Ok("SettingChanged")
                && let Ok(parameters) = values[3].get::<glib::Variant>()
                && let Some((namespace, key, value)) =
                    parameters.get::<(String, String, glib::Variant)>()
            {
                apply(&effects, &namespace, &key, &value);
            }
            None
        });
        let namespaces = (vec![DESKTOP, APPEARANCE],).to_variant();
        if let Ok(reply) = portal
            .call_future("ReadAll", Some(&namespaces), gio::DBusCallFlags::NONE, 1500)
            .await
            && let Some((entries,)) = reply.get::<(PortalSettings,)>()
        {
            for (namespace, values) in entries {
                for (key, value) in values {
                    apply(&settings, &namespace, &key, &value);
                }
            }
        }
        if let Some(app) = app.upgrade() {
            app.connect_shutdown(move |_| {
                let _ = &portal;
            });
        }
    });
}

fn apply(settings: &gtk::Settings, namespace: &str, key: &str, value: &glib::Variant) {
    match (namespace, key) {
        (DESKTOP, "gtk-theme") => {
            let Some(name) = value.str().filter(|name| valid_name(name)) else {
                return;
            };
            let selected = if theme_available(name) {
                name
            } else {
                "Adwaita"
            };
            if settings.gtk_theme_name().as_deref() != Some(selected) {
                settings.set_gtk_theme_name(Some(selected));
            }
        }
        (APPEARANCE, "color-scheme") => {
            let Some(preference @ 0..=2) = value.get::<u32>() else {
                return;
            };
            apply_appearance(settings, preference);
        }
        _ => return,
    }
    if std::env::var_os("DRAGON_DRIVER_THEME_TRACE").is_some() {
        eprintln!(
            "GTK {}.{} theme {:?}, appearance {}",
            gtk::major_version(),
            gtk::minor_version(),
            settings.gtk_theme_name(),
            appearance_name(settings)
        );
    }
}

fn apply_appearance(settings: &gtk::Settings, preference: u32) {
    const MODERN: &str = "gtk-interface-color-scheme";
    if let Some(property) = settings.find_property(MODERN)
        && let Some(values) = glib::EnumClass::with_type(property.value_type())
    {
        let nick = match preference {
            1 => "dark",
            2 => "light",
            _ => "default",
        };
        if let Some(value) = values.value_by_nick(nick) {
            settings.set_property_from_value(MODERN, &value.to_value(&values));
            return;
        }
    }
    if preference == 0 {
        settings.reset_property("gtk-application-prefer-dark-theme");
    } else {
        settings.set_gtk_application_prefer_dark_theme(preference == 1);
    }
}

fn appearance_name(settings: &gtk::Settings) -> String {
    const MODERN: &str = "gtk-interface-color-scheme";
    if settings.find_property(MODERN).is_some()
        && let Some((_, value)) = glib::EnumValue::from_value(&settings.property_value(MODERN))
    {
        return value.nick().to_owned();
    }
    if settings.is_gtk_application_prefer_dark_theme() {
        "dark"
    } else {
        "light"
    }
    .into()
}

fn valid_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 128 && Path::new(name).file_name() == Some(OsStr::new(name))
}

fn theme_available(name: &str) -> bool {
    if matches!(name, "Adwaita" | "HighContrast" | "Default") {
        return true;
    }
    let roots = std::iter::once(glib::user_data_dir())
        .chain(glib::system_data_dirs())
        .map(|path| path.join("themes"))
        .chain([
            glib::home_dir().join(".themes"),
            glib::home_dir().join(".local/share/themes"),
        ]);
    roots
        .filter_map(|root| std::fs::read_dir(root.join(name)).ok())
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| {
            entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.starts_with("gtk-4."))
                && entry.path().join("gtk.css").is_file()
        })
}
