# GTK application entry point

`APP_NAME` supplies the window title and header label. `APP_ID` matches the desktop entry, package metadata and window icon. `main` owns the GTK application and its main context. It calls `ui::install_style` at startup and presents the existing window or opens a new one at activation. The [GTK window](ui/mod.md) keeps HID and file work on a separate worker thread.
