# Distribution staging

`stage-distribution.sh` takes the build output directory and the staging directory. Run it inside the project build container so the frontend binaries and GTK dependencies come from the same runtime. It strips staged copies, leaves the original binaries intact, adds desktop metadata and the wallpaper resizer's license, and makes the separate host service payload. The AppImage includes the linked GTK libraries while the host supplies glibc and graphics drivers.

The approved violet dragon and ring in `org.dragondriver.DragonDriver.svg` are the master app icon. Both packages install it as the scalable desktop icon. The AppImage also uses a transparent 128 × 128 PNG for `.DirIcon`. Regenerate that file from the master with Inkscape's `--export-width=128` option when the artwork changes.
