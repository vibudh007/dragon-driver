# Dragon Driver code audit

## Scope and evidence

The audit covers the Rust GUI, desktop and system daemons, CLI, keyboard backend, SDK, Rust diagnostic tools, and package scripts in this workspace. The function index provides source locations by module, including fixtures and native declarations. Windows binaries and firmware are reference inputs.

Pass 1 checked resource ownership, externally supplied lengths and indices, worker and daemon lifetime, lock scope, duplicated computation, and errors during startup, transfer, and teardown. The reference build passed the default offline test suite and normal Clippy checks. The additional lint pass flagged fixed packet slices, intentional vendor counter truncation, eager cleanup, clones, and temporary lock lifetimes. Packet layout and integer wrapping were checked against their guards and existing fixtures.

## Pass 1 findings and fixes

| ID | Functions | Root cause and observable risk | Fix and expected behavior |
| --- | --- | --- | --- |
| DD-01 | `reactive::Run::wallpaper` | The temporary inbox lock in the `if let` scrutinee lived through the HID transfer and any wait for the configuration lock. A slow transfer blocked publication of new decoded wallpaper frames. | Take the pending frame on its own statement. The producer can publish while the consumer waits for HID. The latest-frame policy and keepalive are unchanged. |
| DD-02 | `Palettes::set_slot` | Equality of RGB bytes alone suppressed confirmation for a different last-used slot or a different observed effect. | Skip a write only when the slot bytes and all metadata that would change already match. Confirmed colors still select their slot and Custom mode, and an interrupted write still invalidates the affected frame. |
| DD-03 | `CliTransport::drop`, `serve_device` | The parent killed the relay immediately after signaling EOF. That could interrupt existing synchronization cleanup. | Give the relay up to 200 ms to perform cleanup and exit. Reap a normal exit and retain forced termination for a stuck relay. The shutdown path runs off the GTK thread. |
| DD-04 | `Spectrum::new`, `Spectrum::transform`, `fft_phases`, `fft` | Each group in every FFT stage regenerated the same phase coefficients for every audio frame. | Generate each stage's sequence once per spectrum using the original floating-point recurrence. Reuse the sequence in every group. Spectrum and bar thresholds remain the same. |
| DD-05 | `KeyMap::default`, `label`, `replace_labels` | Each default key map allocated a tree and copied every constant label. GUI selection and assignment views repeatedly create default maps. | Look up labels in the sorted static table. An explicit replacement still replaces the complete table, including omitted labels. |
| DD-06 | `Device::tick`, `Keyboard::has_desktop_bindings` | Focus-only polling opened and locked the configuration file every 35 ms before discovering there were no desktop keyboard bindings to read. | Check for configured bindings before opening the lock. Focus polling and delivery retain their existing behavior. |
| DD-07 | `service::config_guard`, `prepare_config_lock`, `open_config_file` | Opening a FIFO at the configuration lock path could block before the lock call. Symlinks could redirect the lock to a different file. | Share a regular-file check between publisher and client. Open the path with no symlink following and nonblocking file open, and acquire the same blocking advisory lock only on a regular descriptor. New lock files use mode 0600. |
| DD-08 | `Profiles::contains_advanced_index`, `ui::profiles::actions::store_advanced` | A missing advanced container ended the search before subsequent containers. A partial document could allocate an index already used by another action type. | Search each container that exists. Existing declared indices stay reserved and the editor uses its original first-free-index rule. |
| DD-09 | `ui::desktop::x11::X11::property`, `property_size` | The X11 item count was bounded against the requested maximum, but never checked against the variable-length wire payload before creating an FFI slice. | Require the declared byte count to fit both the request bound and reply payload. Reuse the allocated GTK application ID string when trimming trailing NUL bytes. |
| DD-10 | `Reactive::shutdown`, `handle_with` | A client already accepted by the daemon could submit Start after shutdown inspected the current job. The new worker could then be terminated during startup. | Set a shutdown flag under the same mutex that admits workers. Existing state can still be queried and no new effect can start once teardown begins. |

## Pass 2 review

| Area | Preserved contracts and failure paths |
| --- | --- |
| GTK, readbacks, controls, desktop actions | GTK callbacks continue on the main context. Hardware work stays on the device worker. Revision checks discard stale replies, destroyed views use weak references for delayed callbacks, and travel and calibration keep their existing stop paths. |
| User daemon, privileged broker, reactive workers | Socket peer credentials and request size checks precede data handling. Shared leases still follow the descriptor and configuration lock across GUI and daemon clients. Client closure leaves an authorized reactive effect running. Shutdown excludes new workers and requests existing cleanup. |
| HID discovery and SDK transport | Device identity, report ID, descriptor length checks, packet bytes, deadlines, retry counts, short-write errors and report padding retain their existing wire behavior. Cleanup runs on error as well as successful relay exit. |
| Profile parsing and upload | XML depth, node, byte and entity budgets remain in place. Packet ordering, maximum slot counts, acknowledgement handling and partial-transfer errors retain the vendor contract. Only partial-document index lookup changed. |
| Lighting and media | Firmware modes, frame order, timing, palette fallback, supported image formats, and receiver output are unchanged. Audio phase factors retain the old operation order. |
| Packaging and diagnostic helpers | Both package formats keep the same policies and entry points. Shell scripts received syntax checks. Vendor fixture tools and physical diagnostics remain opt-in and were not executed against hardware. |

The audio comparison used a deterministic 8 MiB PCM input and alternated old and new implementations over five optimized runs. All 4,095 bar frames matched on every run. Median elapsed time dropped from 135.2 ms to 75.5 ms, a 44% reduction in spectrum processing for this fixture. A live 2,048-point spectrum uses approximately 32 KiB of additional reusable phase coefficients. This measurement excludes PipeWire capture, scheduling, and USB writes.

## Verification

- All 126 default offline tests passed. Both GTK test entry points also passed under Xvfb. The physical K617M entry point skips device operations without an explicit live-test configuration.
- The daemon-only feature set passed 110 tests. The no-default-features configuration passed 88 tests. Formatting, Clippy with warnings denied, and the package shell syntax checks passed.
- The new slot confirmation, partial advanced XML, relay EOF cleanup, and late reactor startup regressions were run against a separate copy of the pre-audit source. Each failed on that copy and passed on the revised source. Additional fixtures exercise the configuration lock file types and shortened X11 payloads.
- Hardware lighting, disconnection timing on physical devices, and long-running memory profiling are outside the verified results. Installed packages and services continued to use their existing build.
