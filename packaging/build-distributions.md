# Local x86-64 builds

`build-distributions.sh` creates `Dragon-Driver-x86_64.AppImage`, `dragon-driver-x86_64.flatpak`, `dragon-driver-host-x86_64.tar.gz`, and SHA-256 checksums in `target/dist`. It builds the GUI, CLI, and daemon with Cargo's optimized release profile in the project Podman image using the locked offline Cargo cache. It stages files in a disposable output directory and replaces deliverables only after packaging succeeds. The Flatpak uses the GNOME 49 platform. Its build includes the installed adw-gtk3 light and dark GTK 4 variants with the LGPL license when those files are present. The standard GTK 3 Flatpak extension supplies GTK 3 styles. [Theme source](https://github.com/lassekongo83/adw-gtk3)

Place the official x86-64 appimagetool executable at `target/package-tools/appimagetool` and the x86-64 Type 2 runtime at `target/package-tools/runtime-x86_64`. Verify each digest using the corresponding GitHub release asset metadata before making the tool executable. Install the GNOME 49 Flatpak runtime through the workstation's usual Flatpak setup. Run `packaging/build-distributions.sh` from the project.

[AppImage tool releases](https://github.com/AppImage/appimagetool/releases) · [Type 2 runtime releases](https://github.com/AppImage/type2-runtime/releases) · [Flatpak single-file bundles](https://docs.flatpak.org/en/latest/single-file-bundles.html)
