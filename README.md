# Dragon Driver

Dragon Driver is a native Linux app for configuring compatible Redragon keyboards. It’s written in Rust, uses GTK 4, and follows your system theme.

The wired Redragon K617M Fizz Rapid Trigger is the reference device and has been tested on hardware. Dragon Driver uses the keyboard’s reported capabilities to decide which controls to show. Support for additional models varies by model and firmware.

## Features

- Actuation and Rapid Trigger — Adjust settings per key, track key travel live, and calibrate supported keyboards.
- RGB lighting — Paint individual keys, save two custom palettes, and configure built-in effects. Color, rainbow, speed, and direction controls appear where the effect supports them.
- Wallpaper-reactive lighting — Match the keys to the wallpaper on your active primary display.
- Audio-reactive lighting — Turn desktop playback into a keyboard spectrum. On USB, choose four colors or derive them from your wallpaper. Supported 2.4 GHz receivers use their built-in colors.
- Profiles and bindings — Edit onboard layers, macros, and supported advanced key actions. Import and export XML profiles.
- Command line — Use `dragonctl` to inspect and configure your keyboard from a terminal or shell script.

## Install

The [GitHub Releases](../../releases) page provides x86-64 AppImage and Flatpak builds. Both include the GUI, CLI, and daemon.

### AppImage

```sh
chmod +x Dragon-Driver-x86_64.AppImage
./Dragon-Driver-x86_64.AppImage
```

### Flatpak

The Flatpak requires the GNOME 49 runtime. Install the bundle and open Dragon Driver from your app launcher.

```sh
flatpak install --user --bundle ./dragon-driver-x86_64.flatpak
```

On first launch, Dragon Driver asks for your administrator password, typically the same password you use for `sudo`, to register its keyboard-access service with systemd. After setup, the GUI and desktop daemon run as your normal user. The Flatpak uses host-command permission to launch the bundled installer. [Setup and removal](packaging/setup-desktop-service.md)

## Limitations and requirements

- The first connection overwrites Custom 1 with solid red. The firmware provides no verified per-key RGB readback, so Dragon Driver establishes a known palette and tracks subsequent edits on your PC. Changes made directly on the keyboard can leave the saved palette out of date. Deleting the saved record causes first-connection setup to run again.
- Keyboards of the same model share their saved lighting settings on a PC. Dragon Driver treats them as one model and has no independent per-unit palettes or reactive preferences.
- Audio-reactive RGB requires PipeWire and the host `pw-record` tool.
- Wallpaper-reactive RGB works only on KDE Plasma, GNOME, and COSMIC. It requires a compatible wired keyboard and a readable local wallpaper in PNG, JPEG, or WebP format.
- Some desktop bindings depend on your session. Automatic text paste and foreground profile switching use X11. Native Wayland offers Copy text for text bindings.
- Firmware updates use the vendor’s updater. Dragon Driver covers keyboard configuration.

## What stays saved

The daemon keeps reactive lighting running after you close the app. After you log in again, it restores saved custom palettes and built-in effects, then resumes the reactive mode if you left it enabled. Turning a reactive switch off saves that choice too.

Lighting data lives in `$XDG_DATA_HOME/dragon-driver/palettes.ron`, usually `~/.local/share/dragon-driver/palettes.ron`. [How lighting is saved and restored](src/palettes.md)

## Command line

```sh
./Dragon-Driver-x86_64.AppImage --cli --help
flatpak run --command=dragonctl org.dragondriver.DragonDriver --help
```

Use `dragonctl COMMAND --help` for an individual command. Many read commands support `--tsv` for scripting. Turn off reactive lighting and close the GUI before running a standalone CLI command that needs exclusive keyboard access. [CLI reference](src/bin/dragonctl.md)

## Build from source

Install Rust 1.90 or newer, a system linker, `pkg-config`, GTK 4.8 or newer development files, and the X11 and XCB development libraries.

```sh
cargo build --locked --bins
cargo test --locked --all-targets
target/debug/dragon-driver
```

The repository includes a Podman `Containerfile` and scripts for building the AppImage, Flatpak, and daemon companion archive. [Distribution builds](packaging/build-distributions.md) · [Code audit and verification](AUDIT.md)

## License and credits

Dragon Driver is licensed under the GNU General Public License v3. [GPLv3 license](LICENSE)

Dragon Driver is an independent project based on reverse engineering Redragon’s Windows configuration software. Wallpaper discovery and color processing draw on [Wallpaper Aura](https://github.com/vibudh007/wallpaper-aura).
