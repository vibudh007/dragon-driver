# Configuration transport for dragonctl

`CliTransport::open` starts the installed sibling CLI with a private connected socket as stdin and stdout. It keeps the child owned by the session and closes and reaps it when the session ends. The GUI creates requests with the shared Rust SDK. The CLI receives read, fixed-length report write, bounded wait, and nonblocking commands and performs all actual HID I/O through a daemon-issued descriptor.

`serve` verifies the requested node against the discovered catalog and joins the desktop daemon’s shared descriptor lease. It briefly retries a lease held by the idle observer. The GUI worker holds the configuration lock around a job, and the daemon takes that same lock for reactive transfers. `serve_device` preserves the exact report bytes and returned lengths. Replies carry an error code before any payload. Input EOF ends the session. The relay test uses an isolated socket pair and never opens a keyboard.

`relay_reports` remembers whether the frontend entered live lighting synchronization. If the frontend exits with the lighting stream still active, `serve_device` attempts to leave synchronization before releasing the descriptor. The disconnect test checks the outgoing exit report.

Dropping the transport signals EOF and allows up to 200 ms for the relay to leave synchronization and exit. A relay that stays alive is killed and reaped.
