use std::cell::Cell;
use std::collections::HashMap;
use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::{fs::OpenOptionsExt, net::UnixStream};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use gio::{glib, prelude::*};
use serde::Deserialize;

use crate::cosmic_outputs::Outputs;
use crate::service::{self, MAX_WALLPAPER_BYTES};

const PLASMA: &str = "org.kde.plasmashell";
const PLASMA_PATH: &str = "/PlasmaShell";
// Plasma's screen pool assigns index 0 to the first (primary) active output.
const PLASMA_PRIMARY: u32 = 0;
const MUTTER: &str = "org.gnome.Mutter.DisplayConfig";
const MUTTER_PATH: &str = "/org/gnome/Mutter/DisplayConfig";
const COSMIC_CONFIG: &str = "cosmic/com.system76.CosmicBackground/v1";
const CONFIG_LIMIT: usize = 128 * 1024;

enum Source {
    Cosmic {
        state: PathBuf,
        config: PathBuf,
        outputs: Option<Box<Outputs>>,
    },
    Gnome {
        background: gio::Settings,
        desktop: Option<gio::Settings>,
        bus: Option<gio::DBusConnection>,
    },
    Plasma {
        config: PathBuf,
        bus: gio::DBusConnection,
    },
}

impl Source {
    fn detect(context: &glib::MainContext) -> io::Result<Self> {
        let desktop = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .to_ascii_lowercase();
        if desktop.split(':').any(|name| name == "cosmic") {
            return Ok(Self::Cosmic {
                state: std::env::var_os("XDG_STATE_HOME")
                    .map(PathBuf::from)
                    .filter(|path| path.is_absolute())
                    .unwrap_or_else(|| glib::home_dir().join(".local/state"))
                    .join(COSMIC_CONFIG)
                    .join("wallpapers"),
                config: glib::user_config_dir().join(COSMIC_CONFIG),
                outputs: Outputs::open(context).ok().map(Box::new),
            });
        }
        if desktop
            .split(':')
            .any(|name| matches!(name, "kde" | "plasma"))
        {
            return Ok(Self::Plasma {
                config: glib::user_config_dir().join("plasma-org.kde.plasma.desktop-appletsrc"),
                bus: gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE)
                    .map_err(io::Error::other)?,
            });
        }
        let background = settings("org.gnome.desktop.background").ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "Wallpaper discovery needs COSMIC, GNOME or KDE Plasma",
            )
        })?;
        Ok(Self::Gnome {
            background,
            desktop: settings("org.gnome.desktop.interface"),
            bus: gio::bus_get_sync(gio::BusType::Session, gio::Cancellable::NONE).ok(),
        })
    }

    fn path(&self) -> io::Result<Option<PathBuf>> {
        match self {
            Self::Cosmic {
                state,
                config,
                outputs,
            } => {
                let primary = outputs.as_ref().and_then(|outputs| outputs.primary());
                if outputs.is_some() && primary.is_none() {
                    return Ok(None);
                }
                if let Some(path) = shared_cosmic_path(config)? {
                    return Ok(Some(path));
                }
                cosmic_path(&read_regular(state, CONFIG_LIMIT)?, primary)
            }
            Self::Gnome {
                background,
                desktop,
                bus,
            } => {
                // GNOME applies the same wallpaper to all active logical monitors.
                if bus.as_ref().and_then(gnome_has_display) == Some(false) {
                    return Ok(None);
                }
                if background.value("picture-options").str() == Some("none") {
                    return Ok(None);
                }
                let dark = desktop.as_ref().is_some_and(|settings| {
                    settings
                        .settings_schema()
                        .is_some_and(|schema| schema.has_key("color-scheme"))
                        && settings.string("color-scheme") == "prefer-dark"
                });
                let key = if dark
                    && background
                        .settings_schema()
                        .is_some_and(|schema| schema.has_key("picture-uri-dark"))
                {
                    "picture-uri-dark"
                } else {
                    "picture-uri"
                };
                let uri = background.string(key);
                if uri.is_empty() {
                    Ok(None)
                } else {
                    local_path(&uri).map(Some)
                }
            }
            Self::Plasma { bus, .. } => {
                let response = bus
                    .call_sync(
                        Some(PLASMA),
                        PLASMA_PATH,
                        "org.kde.PlasmaShell",
                        "wallpaper",
                        Some(&(PLASMA_PRIMARY,).to_variant()),
                        None,
                        gio::DBusCallFlags::NONE,
                        1500,
                        gio::Cancellable::NONE,
                    )
                    .map_err(io::Error::other)?;
                let values = response
                    .get::<(HashMap<String, glib::Variant>,)>()
                    .ok_or_else(invalid)?
                    .0;
                values
                    .get("Image")
                    .and_then(glib::Variant::str)
                    .map(local_path)
                    .transpose()
            }
        }
    }

    fn watch(
        &self,
        dirty: &Rc<Cell<bool>>,
    ) -> (Vec<gio::FileMonitor>, Option<gio::SignalSubscription>) {
        match self {
            Self::Cosmic { state, config, .. } => (
                [
                    state.clone(),
                    config.join("same-on-all"),
                    config.join("all"),
                ]
                .iter()
                .filter_map(|path| monitor(path, dirty))
                .collect(),
                None,
            ),
            Self::Plasma { config, bus } => {
                let updated = dirty.clone();
                let signals = bus.subscribe_to_signal(
                    Some(PLASMA),
                    None,
                    None,
                    Some(PLASMA_PATH),
                    None,
                    gio::DBusSignalFlags::NONE,
                    move |_| updated.set(true),
                );
                let config_watches = [
                    config.clone(),
                    glib::user_config_dir().join("kwinoutputconfig.json"),
                ]
                .iter()
                .filter_map(|path| monitor(path, dirty))
                .chain(
                    gio::File::for_path(glib::user_data_dir().join("kscreen"))
                        .monitor_directory(
                            gio::FileMonitorFlags::WATCH_MOVES,
                            gio::Cancellable::NONE,
                        )
                        .ok()
                        .map(|watch| watch_file_events(watch, dirty)),
                )
                .collect();
                (config_watches, Some(signals))
            }
            Self::Gnome {
                background,
                desktop,
                bus,
            } => {
                let updated = dirty.clone();
                background.connect_changed(None, move |_, _| updated.set(true));
                if let Some(desktop) = desktop {
                    let updated = dirty.clone();
                    desktop.connect_changed(Some("color-scheme"), move |_, _| updated.set(true));
                }
                let signals = bus.as_ref().map(|bus| {
                    let updated = dirty.clone();
                    bus.subscribe_to_signal(
                        Some("org.gnome.Mutter.DisplayConfig"),
                        Some(MUTTER),
                        Some("MonitorsChanged"),
                        Some(MUTTER_PATH),
                        None,
                        gio::DBusSignalFlags::NONE,
                        move |_| updated.set(true),
                    )
                });
                (Vec::new(), signals)
            }
        }
    }

    fn refresh_outputs(&mut self) -> bool {
        if let Self::Cosmic { outputs, .. } = self
            && let Some(watch) = outputs
        {
            match watch.refresh() {
                Ok(changed) => return changed,
                Err(_) => *outputs = None,
            }
            return true;
        }
        false
    }
}

fn gnome_has_display(bus: &gio::DBusConnection) -> Option<bool> {
    let state = bus
        .call_sync(
            Some(MUTTER),
            MUTTER_PATH,
            MUTTER,
            "GetCurrentState",
            None,
            None,
            gio::DBusCallFlags::NO_AUTO_START,
            500,
            gio::Cancellable::NONE,
        )
        .ok()?;
    Some(state.try_child_value(2)?.n_children() != 0)
}

fn settings(name: &str) -> Option<gio::Settings> {
    let schema = gio::SettingsSchemaSource::default()?.lookup(name, true)?;
    Some(gio::Settings::new_full(
        &schema,
        gio::SettingsBackend::NONE,
        None,
    ))
}

#[derive(Deserialize)]
enum CosmicWallpaper {
    Path(String),
    Color(#[allow(dead_code)] ron::Value),
}

#[derive(Deserialize)]
struct CosmicBackground {
    source: CosmicWallpaper,
}

fn shared_cosmic_path(config: &Path) -> io::Result<Option<PathBuf>> {
    let shared = read_regular(&config.join("same-on-all"), CONFIG_LIMIT)
        .ok()
        .and_then(|bytes| ron::de::from_bytes::<bool>(&bytes).ok())
        .unwrap_or(false);
    if !shared {
        return Ok(None);
    }
    let bytes = read_regular(&config.join("all"), CONFIG_LIMIT)?;
    let background: CosmicBackground = ron::de::from_bytes(&bytes).map_err(io::Error::other)?;
    match background.source {
        CosmicWallpaper::Path(path) => {
            let path = local_path(&path)?;
            Ok((!path.is_dir()).then_some(path))
        }
        CosmicWallpaper::Color(_) => Err(invalid()),
    }
}

fn cosmic_path(bytes: &[u8], primary: Option<&str>) -> io::Result<Option<PathBuf>> {
    let sources: Vec<(String, CosmicWallpaper)> =
        ron::de::from_bytes(bytes).map_err(io::Error::other)?;
    let selected = match primary {
        Some(primary) => sources.iter().find(|(name, _)| name == primary),
        None if sources.len() == 1 => sources.first(),
        None => None,
    };
    match selected {
        Some((_, CosmicWallpaper::Path(path))) => local_path(path).map(Some),
        _ => Ok(None),
    }
}

fn local_path(value: &str) -> io::Result<PathBuf> {
    let path = if value.starts_with("file:///") || value.starts_with("file://localhost/") {
        if value.contains(['?', '#', '\0']) {
            return Err(invalid());
        }
        gio::File::for_uri(value).path().ok_or_else(invalid)?
    } else {
        PathBuf::from(value)
    };
    if !path.is_absolute() || path.as_os_str().as_encoded_bytes().contains(&0) {
        return Err(invalid());
    }
    Ok(path)
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "Wallpaper must be a local image file",
    )
}

fn read_regular(path: &Path, limit: usize) -> io::Result<Vec<u8>> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.len() > limit as u64 {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > limit {
        return Err(invalid());
    }
    Ok(bytes)
}

fn monitor(path: &Path, dirty: &Rc<Cell<bool>>) -> Option<gio::FileMonitor> {
    let monitor = gio::File::for_path(path)
        .monitor_file(gio::FileMonitorFlags::WATCH_MOVES, gio::Cancellable::NONE)
        .ok()?;
    Some(watch_file_events(monitor, dirty))
}

fn watch_file_events(monitor: gio::FileMonitor, dirty: &Rc<Cell<bool>>) -> gio::FileMonitor {
    monitor.set_rate_limit(0);
    let updated = dirty.clone();
    monitor.connect_changed(move |_, _, _, event| {
        use gio::FileMonitorEvent::*;
        if matches!(
            event,
            Changed | ChangesDoneHint | Created | Deleted | MovedIn | MovedOut | Renamed
        ) {
            updated.set(true);
        }
    });
    monitor
}

pub fn serve(mut stream: UnixStream) -> io::Result<()> {
    let context = glib::MainContext::new();
    context
        .with_thread_default(|| {
            let source = match Source::detect(&context) {
                Ok(source) => source,
                Err(error) => return service::response(&mut stream, Err(error)),
            };
            service::response(&mut stream, Ok(()))?;
            relay_watched(&mut stream, &context, source)
        })
        .map_err(io::Error::other)?
}

fn relay_watched(
    stream: &mut UnixStream,
    context: &glib::MainContext,
    mut source: Source,
) -> io::Result<()> {
    let dirty = Rc::new(Cell::new(true));
    let _watch = source.watch(&dirty);
    let closed = Arc::new(AtomicBool::new(false));
    let flag = closed.clone();
    let disconnect = glib::source::unix_fd_source_new(
        stream.as_raw_fd(),
        glib::IOCondition::IN | glib::IOCondition::HUP | glib::IOCondition::ERR,
        Some("wallpaper-client"),
        glib::Priority::DEFAULT,
        move |_, _| {
            flag.store(true, Ordering::Relaxed);
            glib::ControlFlow::Break
        },
    );
    disconnect.attach(Some(context));
    let result = relay(stream, context, &mut source, &closed, &dirty);
    disconnect.destroy();
    result
}

fn relay(
    stream: &mut UnixStream,
    context: &glib::MainContext,
    source: &mut Source,
    closed: &AtomicBool,
    dirty: &Rc<Cell<bool>>,
) -> io::Result<()> {
    let mut path = None;
    let mut image_watch = None;
    let image_dirty = Rc::new(Cell::new(true));
    while !closed.load(Ordering::Relaxed) {
        if source.refresh_outputs() {
            dirty.set(true);
        }
        let mut changed = image_dirty.replace(false);
        if dirty.replace(false) {
            let current = source.path().ok().flatten();
            if current != path {
                image_watch = current
                    .as_deref()
                    .and_then(|path| monitor(path, &image_dirty));
                path = current;
                changed = true;
            }
        }
        if changed {
            let image = path
                .as_deref()
                .and_then(|path| read_regular(path, MAX_WALLPAPER_BYTES).ok())
                .unwrap_or_default();
            stream.write_all(&(image.len() as u32).to_le_bytes())?;
            stream.write_all(&image)?;
        }
        context.iteration(true);
    }
    drop(image_watch);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selects_the_requested_cosmic_output_and_only_accepts_local_files() {
        let state =
            br#"[("HDMI-1", Path("/other.png")), ("eDP-1", Path("file:///wall%20paper.png"))]"#;
        assert_eq!(
            cosmic_path(state, Some("eDP-1")).unwrap(),
            Some(PathBuf::from("/wall paper.png"))
        );
        assert_eq!(
            cosmic_path(state, Some("HDMI-1")).unwrap(),
            Some("/other.png".into())
        );
        assert_eq!(cosmic_path(state, None).unwrap(), None);
        assert!(local_path("https://example.test/image.png").is_err());
        assert!(local_path("file://remote/image.png").is_err());
    }

    #[test]
    fn relays_wallpaper_replacement_and_stops_on_disconnect() {
        use std::{fs, thread, time::Duration};

        let directory = std::env::temp_dir().join(format!(
            "dragon-driver-wallpaper-test-{}",
            std::process::id()
        ));
        fs::create_dir(&directory).unwrap();
        struct Fixture(PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let fixture = Fixture(directory);
        let config = fixture.0.join("wallpapers");
        let first = fixture.0.join("first.png");
        let second = fixture.0.join("second.png");
        let select = |path: &Path| format!("[({:?}, Path({:?}))]", "eDP-1", path);
        fs::write(&first, b"first image").unwrap();
        fs::write(&second, b"second image").unwrap();
        fs::write(&config, select(&first)).unwrap();
        let (mut sender, mut client) = UnixStream::pair().unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        let watched_config = config.clone();
        let settings = fixture.0.clone();
        let relay = thread::spawn(move || {
            let context = glib::MainContext::new();
            let source = Source::Cosmic {
                state: watched_config,
                config: settings,
                outputs: None,
            };
            context
                .with_thread_default(|| relay_watched(&mut sender, &context, source))
                .unwrap()
        });
        let mut receive = || {
            let mut length = [0; 4];
            client.read_exact(&mut length).unwrap();
            let mut image = vec![0; u32::from_le_bytes(length) as usize];
            client.read_exact(&mut image).unwrap();
            image
        };
        assert_eq!(receive(), b"first image");
        let new_config = fixture.0.join("new-config");
        fs::write(&new_config, select(&second)).unwrap();
        fs::rename(new_config, config).unwrap();
        assert_eq!(receive(), b"second image");
        let new_image = fixture.0.join("new-image");
        fs::write(&new_image, b"updated image").unwrap();
        fs::rename(new_image, &second).unwrap();
        assert_eq!(
            (0..4)
                .map(|_| receive())
                .find(|image| image != b"second image"),
            Some(b"updated image".to_vec())
        );
        fs::write(second, b"in-place update").unwrap();
        assert_eq!(
            (0..4)
                .map(|_| receive())
                .find(|image| image == b"in-place update"),
            Some(b"in-place update".to_vec())
        );
        fs::write(fixture.0.join("all"), format!("(source: Path({first:?}))")).unwrap();
        fs::write(fixture.0.join("same-on-all"), "true").unwrap();
        assert_eq!(
            (0..6)
                .map(|_| receive())
                .find(|image| image == b"first image"),
            Some(b"first image".to_vec())
        );
        client
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        fs::write(fixture.0.join("wallpapers"), select(&first)).unwrap();
        let mut byte = [0_u8];
        let error = client.read_exact(&mut byte).unwrap_err();
        assert!(matches!(
            error.kind(),
            io::ErrorKind::TimedOut | io::ErrorKind::WouldBlock
        ));
        drop(client);
        relay.join().unwrap().unwrap();
    }
}
