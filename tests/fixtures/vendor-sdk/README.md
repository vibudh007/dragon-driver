# Original SDK fixtures

The offline Rust fixture generator ran the original `witmodSdk.dll` in an isolated 32-bit Wine container with no USB passthrough or network.

- `parameters.xml` and `advanced.xml` preserve the vendor's profile and advanced-key defaults.
- `custom-rgb.xml`, `light-config.xml`, and `onboard-actions.xml` preserve the original output for synthetic onboard RGB, light, remap, and macro records.
- The standard preview binaries contain consecutive 24-byte frames. The six records in each frame give red, green, blue, and intensity. Keyboard records sample six positions in the 6 × 22 grid. Side records cover six synthetic cells.

- `keyboard-reconfigure.bin` has 53 frames of 30 bytes. Its six cell records add palette progress to RGB and intensity. The first five frames are mode 15 and the rest follow a live reconfiguration to mode 16 inside Wine.
- `side-collision-config.bin` holds the five exported collision bytes per frame for 530 mono and 530 rainbow frames.
- `active-rows.bin` records one row-count byte per tick. `automatic-config.bin` records the first cell's trigger flag followed by five little-endian words. On flagged frames the generator reads and acknowledges the automatic ripple report.
- The named 68-, 82-, 98-key and K665 side files cover additional geometry branches.

Rust tests compare the XML record values and every recorded preview byte. The captures characterize preview state rather than physical LED output.
