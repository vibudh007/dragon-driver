#[cfg(feature = "daemon")]
mod cosmic_outputs;
pub mod keyboard;
#[cfg(feature = "daemon")]
pub mod palettes;
#[cfg(feature = "daemon")]
pub mod service;
#[cfg(feature = "daemon")]
pub mod wallpaper_source;
pub mod witmod_sdk;

#[cfg(feature = "daemon")]
mod audio_palette;
#[cfg(feature = "daemon")]
pub mod reactive;
#[cfg(feature = "daemon")]
mod wallpaper_colors;
