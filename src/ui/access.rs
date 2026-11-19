use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

use gtk::{gio, glib};

const HOST_SETUP: &str = "share/dragon-driver/host/setup-desktop-service.sh";
const FLATPAK_INFO: &str = "/.flatpak-info";
const FLATPAK_SPAWN: &str = "/usr/bin/flatpak-spawn";

fn native_installer() -> Option<(PathBuf, PathBuf)> {
    let executable = std::env::current_exe().ok()?;
    let frontend = std::env::var_os("APPIMAGE")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute() && path.is_file())
        .unwrap_or_else(|| executable.clone());
    let packaged = executable.parent()?.parent()?.join(HOST_SETUP);
    if packaged.is_file() {
        return Some((packaged, frontend));
    }
    let script = executable.ancestors().find_map(|directory| {
        let script = directory.join("packaging/setup-desktop-service.sh");
        script.is_file().then_some(script)
    })?;
    Some((script, frontend))
}

fn flatpak_installer(info: &glib::KeyFile) -> Option<PathBuf> {
    if info.string("Application", "name").ok()?.as_str() != crate::APP_ID {
        return None;
    }
    let directory = info.string("Instance", "app-path").ok()?;
    let directory = Path::new(directory.as_str());
    directory.is_absolute().then(|| directory.join(HOST_SETUP))
}

fn setup_command() -> Option<Vec<OsString>> {
    if Path::new(FLATPAK_INFO).is_file() {
        if !Path::new(FLATPAK_SPAWN).is_file() {
            return None;
        }
        let info = glib::KeyFile::new();
        info.load_from_file(FLATPAK_INFO, glib::KeyFileFlags::NONE)
            .ok()?;
        let script = flatpak_installer(&info)?;
        return Some(vec![
            FLATPAK_SPAWN.into(),
            "--host".into(),
            "--watch-bus".into(),
            script.into_os_string(),
            "--flatpak".into(),
        ]);
    }
    if !Path::new("/usr/bin/pkexec").is_file() {
        return None;
    }
    let (script, frontend) = native_installer()?;
    Some(vec![
        script.into_os_string(),
        "--app".into(),
        frontend.into_os_string(),
    ])
}

pub(super) fn can_request() -> bool {
    setup_command().is_some()
}

pub(super) fn request(done: impl FnOnce(Result<(), String>) + 'static) -> io::Result<()> {
    let args = setup_command().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Couldn’t find the Dragon Driver setup program.",
        )
    })?;
    let args: Vec<_> = args.iter().map(OsString::as_os_str).collect();
    let child = gio::Subprocess::newv(&args, gio::SubprocessFlags::STDOUT_SILENCE)
        .map_err(io::Error::other)?;
    child.wait_check_async(gio::Cancellable::NONE, move |result| {
        done(result.map_err(|_| {
            "Keyboard access wasn’t set up. Choose Allow keyboard access to try again.".into()
        }));
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatpak_setup_uses_the_bundle_that_launched_the_app() {
        let info = glib::KeyFile::new();
        info.load_from_data(
            "[Application]\nname=org.dragondriver.DragonDriver\n[Instance]\napp-path=/flatpak test/files\n",
            glib::KeyFileFlags::NONE,
        )
        .unwrap();
        assert_eq!(
            flatpak_installer(&info).unwrap(),
            Path::new("/flatpak test/files").join(HOST_SETUP)
        );
        info.set_string("Instance", "app-path", "relative/flatpak");
        assert!(flatpak_installer(&info).is_none());
        info.set_string("Instance", "app-path", "/unrelated");
        info.set_string("Application", "name", "org.example.Other");
        assert!(flatpak_installer(&info).is_none());
    }
}
