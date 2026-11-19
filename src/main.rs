use gtk::prelude::*;

mod ui;

const APP_NAME: &str = "Dragon Driver";
const APP_ID: &str = "org.dragondriver.DragonDriver";

fn main() -> gtk::glib::ExitCode {
    if std::env::args_os().nth(1).as_deref() == Some(std::ffi::OsStr::new("--gtk-runtime-check")) {
        return if gtk::major_version() == 4 && gtk::minor_version() >= 8 {
            gtk::glib::ExitCode::SUCCESS
        } else {
            gtk::glib::ExitCode::FAILURE
        };
    }
    let app = gtk::Application::builder().application_id(APP_ID).build();
    app.connect_startup(ui::install_style);
    app.connect_activate(|app| {
        if let Some(window) = app.active_window() {
            window.present();
        } else {
            ui::open(app);
        }
    });
    app.run()
}
