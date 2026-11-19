# Dragon Driver service protocol

The desktop socket resides under the current user's runtime directory. The system broker socket uses a numeric UID under `/run/dragon-driver`. Both clients and servers check Linux peer credentials. Requests carry a version, operation and bounded data length or HIDRAW index. A broker verifies the SDK configuration interface before passing a file descriptor. GIO handles descriptor passing and recipients set close-on-exec.

`update_palette` sends a bounded model, firmware and operation to the desktop daemon. `read_saved_palettes` retrieves a read-only RON snapshot. The same protocol works from Flatpak without sharing the host data directory. `require_user_service` probes the installed protocol with no keyboard I/O.

`DesktopAudio` requests the daemon's fixed monitor stream. The reader supports cancellation and closing the connection ends capture.

`DesktopAudio::open_bars` checks the companion capability and subscribes to fixed-size PipeWire spectrum frames. Each frame contains 22 heights from 0 through 6. The legacy `open` entry point still receives PCM.

`require_wallpaper_service` probes the desktop daemon for active-display wallpaper notifications with duplicate settings events filtered out. The daemon also answers the earlier wallpaper capability probes for older clients. `DesktopWallpaper` opens an authenticated subscription and reads length-prefixed local image bytes up to 32 MiB. A zero length reports an unavailable image and leaves the subscription active. Cancellation and EOF end the stream. Only the desktop daemon handles these requests. [Wallpaper discovery](wallpaper_source.md)

`OPEN_SHARED` hands the GUI's CLI transport and the daemon renderer a descriptor from the same validated system-broker lease. The membership socket keeps that lease alive. `config_guard` opens a fresh descriptor for the daemon's runtime lock and holds it throughout a configuration job or a reactive lighting transfer. An independent CLI command continues to request exclusive access.

`REACTIVE` sends a bounded RON request containing the device target and one action. The daemon returns the mode, startup or stop state, optional wallpaper frame and status message. Peer credentials are checked before accessing the request. The command connection can close as soon as the response is received.

`prepare_config_lock` and `config_guard` share a regular-file check on the opened descriptor. The lock endpoint rejects symlinks and does not wait for FIFO open. Regular descriptors retain the same exclusive advisory lock semantics.

The lighting persistence support probe lets the GUI request an update for an older desktop daemon. `PaletteUpdate::Lighting` saves all decoded effect parameters for one region. `PaletteUpdate::ConfirmedSlot` saves the RGB page and confirmed effect together. Existing mode-only and legacy slot messages remain readable.
