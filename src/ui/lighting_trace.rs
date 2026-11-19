use std::fmt::Arguments;
use std::sync::OnceLock;
use std::time::Instant;

pub(super) fn event(message: Arguments<'_>) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    if *ENABLED.get_or_init(|| std::env::var("DRAGON_DRIVER_LIGHT_TRACE").as_deref() == Ok("1")) {
        static STARTED: OnceLock<Instant> = OnceLock::new();
        eprintln!(
            "lighting-trace {:8.3}s {message}",
            STARTED.get_or_init(Instant::now).elapsed().as_secs_f64()
        );
    }
}
