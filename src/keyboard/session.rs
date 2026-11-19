use std::io::{self, Read};
use std::num::NonZeroU32;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::audio::{BAR_COUNT, Spectrum, WINDOW_SAMPLES};
use super::bindings::{ProfilePlan, UploadStep};
use super::desktop::{self, DesktopBinding};
use super::hall::{self, Control, Readback, Tuning};
use super::lighting::{self, ColorTransfer, Effect, Region, RgbFrame, SupportedModes};
use super::models::{self, Layout, Position};
use super::performance::{self, LinkStatus, Sleep, WinLock};
use super::report::{Packet, Reply};
use super::transfer::DirectMapping;
use super::{key_input, wireless};
use crate::witmod_sdk::catalog::UsbId;
use crate::witmod_sdk::discovery::ConfigInterface;
use crate::witmod_sdk::hidraw::{HidrawDevice, WriteError};
use crate::witmod_sdk::identity::Identity;
use crate::witmod_sdk::session::SdkSession;
use std::collections::HashMap;

const REPLY_TIMEOUT: Duration = Duration::from_secs(1);
const DRAIN_LIMIT: usize = 128;
const CUSTOM_LIGHTING_MODE: u8 = 10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum UploadResult {
    Transmitted,
    DeviceAccepted,
}

pub struct Keyboard {
    handle: HidrawDevice,
    usb_id: UsbId,
    identity: Identity,
    layout: &'static Layout,
    response_ready: bool,
    desktop: HashMap<u8, DesktopBinding>,
    interface: ConfigInterface,
    wallpaper: Option<Effect>,
    #[cfg(feature = "daemon")]
    access: crate::witmod_sdk::session::Access,
}

pub struct Calibration<'a> {
    keyboard: &'a mut Keyboard,
    all: bool,
    active: bool,
}

impl Calibration<'_> {
    pub fn progress(&mut self, timeout: Duration) -> io::Result<Option<hall::CalibrationStatus>> {
        let mut report = [0; 64];
        let report_id = self.keyboard.handle.layout().output_id.byte();
        let started = Instant::now();
        loop {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() || !self.keyboard.handle.wait_for_report(remaining)? {
                return Ok(None);
            }
            let count = self.keyboard.handle.read_report(&mut report)?;
            if count > 0 && report[0] == report_id {
                match hall::decode_calibration(&report[..count]) {
                    Some(event @ hall::CalibrationStatus::All { .. }) if self.all => {
                        return Ok(Some(event));
                    }
                    Some(event @ hall::CalibrationStatus::Single(_)) if !self.all => {
                        return Ok(Some(event));
                    }
                    _ => {}
                }
            }
        }
    }

    pub fn stop(mut self) -> io::Result<()> {
        self.leave()
    }

    fn leave(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        let report = if self.all {
            hall::calibration_all(false)
        } else {
            hall::calibration_selection(&[])
        };
        let result = self.keyboard.handle.send(report);
        if result.is_err() {
            self.keyboard.response_ready = false;
        }
        result
    }
}

impl Drop for Calibration<'_> {
    fn drop(&mut self) {
        let _ = self.leave();
    }
}

pub struct TravelMonitor<'a> {
    keyboard: &'a mut Keyboard,
    key: Position,
    active: bool,
}

impl TravelMonitor<'_> {
    pub fn next_reading(&mut self, timeout: Duration) -> io::Result<Option<u8>> {
        let started = Instant::now();
        let mut report = [0; 64];
        let report_id = self.keyboard.handle.layout().output_id.byte();
        loop {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() || !self.keyboard.handle.wait_for_report(remaining)? {
                return Ok(None);
            }
            let count = self.keyboard.handle.read_report(&mut report)?;
            if count != 0
                && report[0] == report_id
                && let Some((position, travel)) = hall::decode_travel(&report[..count])
                && position == self.key
            {
                return Ok(Some(travel));
            }
        }
    }

    pub fn stop(mut self) -> io::Result<()> {
        self.leave()
    }

    fn leave(&mut self) -> io::Result<()> {
        if !self.active {
            return Ok(());
        }
        self.active = false;
        let result = self.keyboard.handle.send(hall::stop_travel());
        if result.is_err() {
            self.keyboard.response_ready = false;
        }
        result
    }
}

impl Drop for TravelMonitor<'_> {
    fn drop(&mut self) {
        let _ = self.leave();
    }
}

impl Keyboard {
    pub fn open(sdk: &SdkSession, device: &ConfigInterface) -> io::Result<Self> {
        let (handle, identity) = sdk.open_identified(device)?;
        Self::from_identified(sdk, device, handle, identity)
    }

    #[cfg(feature = "daemon")]
    pub fn open_broker_file(
        sdk: &SdkSession,
        device: &ConfigInterface,
        file: std::fs::File,
    ) -> io::Result<Self> {
        let (handle, identity) = sdk.identify_broker_file(device, file)?;
        Self::from_identified(sdk, device, handle, identity)
    }

    fn from_identified(
        _sdk: &SdkSession,
        device: &ConfigInterface,
        handle: HidrawDevice,
        identity: Identity,
    ) -> io::Result<Self> {
        let layout = models::from_identity(&identity).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::Unsupported,
                "the keyboard identity has no vendor layout",
            )
        })?;
        Ok(Self {
            handle,
            usb_id: device.usb_id(),
            identity,
            layout,
            response_ready: true,
            desktop: HashMap::new(),
            interface: device.clone(),
            wallpaper: None,
            #[cfg(feature = "daemon")]
            access: _sdk.access(),
        })
    }

    pub fn layout(&self) -> &'static Layout {
        self.layout
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    /// Reopen a desynchronized read session and verify the same device identity.
    pub fn recover_reads(&mut self) -> io::Result<()> {
        if self.response_ready {
            return Ok(());
        }
        if self.has_desktop_bindings() {
            return self.ensure_ready();
        }
        #[cfg(feature = "daemon")]
        let sdk = SdkSession::initialize_with_access(self.access)?;
        #[cfg(not(feature = "daemon"))]
        let sdk = SdkSession::initialize()?;
        let _ = self.stop_wallpaper_lighting();
        self.handle.close();
        let next = Self::open(&sdk, &self.interface)?;
        if next.identity != self.identity {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "the keyboard identity changed",
            ));
        }
        *self = next;
        Ok(())
    }

    pub fn read_link_status(&mut self) -> io::Result<LinkStatus> {
        let received = self.query(performance::status_query(), performance::status_reply())?;
        LinkStatus::decode(received.as_ref())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid link status reply"))
    }

    pub fn read_control(&mut self, control: Control) -> io::Result<u8> {
        self.ensure_magnetic()?;
        let report = self.query(control.request(None), control.reply())?;
        Ok(report.as_ref()[7])
    }

    pub fn actuation_limit(&mut self) -> io::Result<u8> {
        self.ensure_magnetic()?;
        let received = self.query(hall::sensitivity_request(), hall::sensitivity_reply())?;
        let limit = received.as_ref()[7];
        if limit > 0 {
            Ok(limit)
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "keyboard returned a zero actuation limit",
            ))
        }
    }

    pub fn read_key(&mut self, key: Position) -> io::Result<Readback> {
        self.ensure_magnetic()?;
        if self.layout.key(key).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "position has no key",
            ));
        }
        let report = self.query(hall::key_request(key), hall::key_reply())?;
        hall::decode_key(report.as_ref()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid magnetic settings reply",
            )
        })
    }

    pub fn monitor_travel(&mut self, key: Position) -> io::Result<TravelMonitor<'_>> {
        self.ensure_magnetic()?;
        self.ensure_ready()?;
        if self.layout.key(key).is_none() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "position has no key",
            ));
        }
        drain(&mut self.handle, &mut [0; 64])?;
        let monitor = TravelMonitor {
            keyboard: self,
            key,
            active: true,
        };
        monitor.keyboard.send(hall::calibration_selection(&[key]))?;
        Ok(monitor)
    }

    pub fn apply(&mut self, keys: &[Position], tuning: Tuning) -> io::Result<()> {
        let packet = hall::apply(self.layout, keys, tuning)?;
        self.send(packet)
    }

    pub fn disable_rapid_trigger(&mut self, key: Position) -> io::Result<()> {
        let before = self.read_key(key)?;
        if let Some(report) = hall::disable_rapid_report(key, before)? {
            self.send(report)?;
            let after = self.read_key(key)?;
            if after.mode != 0 || after.press != before.press || after.release != before.release {
                return Err(io::Error::other(
                    "keyboard readback did not confirm the requested trigger settings",
                ));
            }
        }
        Ok(())
    }

    pub fn set_control(&mut self, control: Control, value: u8) -> io::Result<()> {
        self.ensure_magnetic()?;
        self.send(control.request(Some(value)))
    }

    pub fn read_lighting(&mut self, region: Region) -> io::Result<Effect> {
        self.ensure_region(region)?;
        region
            .decode(self.query(region.query(), region.reply())?.as_ref())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid lighting reply"))
    }

    pub fn read_lighting_modes(&mut self, region: Region) -> io::Result<SupportedModes> {
        self.ensure_region(region)?;
        region
            .decode_modes(self.query(region.modes(), region.mode_reply())?.as_ref())
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid lighting mode list"))
    }

    pub fn set_lighting(
        &mut self,
        region: Region,
        effect: Effect,
        change_stamp: u16,
    ) -> io::Result<()> {
        self.ensure_region(region)?;
        let packet = region
            .effect(effect, change_stamp)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid lighting level"))?;
        self.send(packet)?;
        if region == Region::Keyboard
            && let Some(wallpaper) = &mut self.wallpaper
        {
            wallpaper.brightness = effect.brightness;
        }
        Ok(())
    }

    pub fn set_static_color(&mut self, rgb: [u8; 3]) -> io::Result<()> {
        let region = Region::Keyboard;
        let current = self.read_lighting(region)?;
        if current.mode == 0 && current.foreground == rgb {
            return Ok(());
        }
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_millis() as u16;
        self.set_lighting(
            region,
            Effect {
                mode: 0,
                foreground: rgb,
                ..current
            },
            stamp,
        )?;
        let after = self.read_lighting(region)?;
        if after.mode == 0 && after.foreground == rgb {
            Ok(())
        } else {
            Err(io::Error::other(
                "keyboard readback did not confirm the requested color",
            ))
        }
    }

    pub fn read_sleep(&mut self) -> io::Result<Sleep> {
        Sleep::decode(
            self.query(performance::sleep_query(), performance::sleep_reply())?
                .as_ref(),
        )
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "invalid sleep reply"))
    }

    pub fn set_sleep(&mut self, sleep: Sleep) -> io::Result<()> {
        self.query(sleep.request(), performance::sleep_write_reply())
            .map(|_| ())
    }

    pub fn set_shortcut_lock(&mut self, lock: WinLock) -> io::Result<()> {
        self.send(lock.request())
    }

    pub fn reset_settings(&mut self) -> io::Result<()> {
        self.clear_desktop_bindings();
        self.send(performance::reset())
    }

    pub fn set_gamepad_mode(&mut self, mode: u8) -> io::Result<()> {
        if self.usb_id.vendor != 0x0416 || !(0x7472..=0x7474).contains(&self.usb_id.product) {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "model has no vendor gamepad mode",
            ));
        }
        if mode > 2 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid gamepad mode",
            ));
        }
        self.clear_desktop_bindings();
        self.send(performance::gamepad_mode(mode))?;
        // The EXE warns that switching modes restarts the USB interface.
        self.response_ready = false;
        Ok(())
    }

    pub fn reset_magnetic_settings(&mut self) -> io::Result<()> {
        self.ensure_magnetic()?;
        self.send(hall::defaults_request())
    }

    pub fn calibrate(&mut self, key: Option<Position>) -> io::Result<Calibration<'_>> {
        self.ensure_magnetic()?;
        self.ensure_ready()?;
        if key.is_some_and(|key| self.layout.key(key).is_none()) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "position has no key",
            ));
        }
        drain(&mut self.handle, &mut [0; 64])?;
        let session = Calibration {
            keyboard: self,
            all: key.is_none(),
            active: true,
        };
        if let Some(key) = key {
            session.keyboard.send(hall::calibration_selection(&[key]))?;
            session.keyboard.send(hall::calibration_start(key))?;
        } else {
            session.keyboard.send(hall::calibration_all(true))?;
        }
        Ok(session)
    }

    pub fn preview_custom_colors(&mut self, frame: &RgbFrame) -> io::Result<()> {
        for packet in lighting::keyboard_frame(ColorTransfer::Custom, frame) {
            self.send(packet)?;
        }
        Ok(())
    }

    pub fn show_wallpaper_lighting(&mut self, frame: &RgbFrame) -> io::Result<()> {
        let result = self.begin_wallpaper_lighting().and_then(|()| {
            for report in lighting::keyboard_frame(ColorTransfer::UsbStream, frame) {
                self.send(report)?;
            }
            Ok(())
        });
        if result.is_err() {
            let _ = self.stop_wallpaper_lighting();
        }
        result
    }

    fn begin_wallpaper_lighting(&mut self) -> io::Result<()> {
        if self.wallpaper.is_some() {
            return Ok(());
        }
        if self.read_link_status()?.transport != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Wallpaper lighting requires a wired connection",
            ));
        }
        let current = self.read_lighting(Region::Keyboard)?;
        self.wallpaper = Some(current);
        if current.mode != CUSTOM_LIGHTING_MODE {
            let activate = lighting::activate_custom(CUSTOM_LIGHTING_MODE, current.brightness, 1)
                .ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid lighting level")
            })?;
            self.send(activate)?;
            if self.read_lighting(Region::Keyboard)?.mode != CUSTOM_LIGHTING_MODE {
                return Err(io::Error::other(
                    "The keyboard did not switch to per-key lighting",
                ));
            }
        }
        self.keep_wallpaper_lighting()
    }

    pub fn keep_wallpaper_lighting(&mut self) -> io::Result<()> {
        if self.wallpaper.is_none() {
            return Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "Wallpaper lighting has stopped",
            ));
        }
        self.send(lighting::synchronize(true))
    }

    pub fn stop_wallpaper_lighting(&mut self) -> io::Result<()> {
        let Some(previous) = self.wallpaper else {
            return Ok(());
        };
        let sync = self.handle.send(lighting::synchronize(false));
        let restore = if previous.mode == CUSTOM_LIGHTING_MODE {
            Ok(())
        } else {
            let stamp = lighting_stamp()?;
            let packet = Region::Keyboard.effect(previous, stamp).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "invalid lighting level")
            })?;
            self.handle.send(packet)
        };
        sync.and(restore)?;
        self.wallpaper = None;
        Ok(())
    }

    pub fn apply_custom_colors(
        &mut self,
        page: u8,
        mode: u8,
        brightness: u16,
        speed: u16,
        frame: &RgbFrame,
    ) -> io::Result<()> {
        let activate = lighting::activate_custom(mode, brightness, speed)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "invalid lighting level"))?;
        self.write_custom_page(page, frame)?;
        self.send(activate)
    }

    pub(crate) fn write_custom_page(&mut self, page: u8, frame: &RgbFrame) -> io::Result<()> {
        for packet in lighting::rgb_frame(ColorTransfer::Custom, page, frame) {
            self.send(packet)?;
        }
        thread::sleep(Duration::from_millis(5));
        Ok(())
    }

    #[cfg(feature = "daemon")]
    pub(crate) fn restore_effect(&mut self, region: Region, effect: Effect) -> io::Result<()> {
        self.set_lighting(region, effect, lighting_stamp()?)?;
        let actual = self.read_lighting(region)?;
        if actual.mode != effect.mode || actual.brightness != effect.brightness {
            return Err(io::Error::other(
                "The keyboard did not confirm its saved lighting",
            ));
        }
        Ok(())
    }

    pub fn send_wireless_lighting(
        &mut self,
        stamp: u16,
        effect: lighting::WirelessEffect,
        side: Option<lighting::WirelessSide>,
    ) -> io::Result<()> {
        if side.is_some() {
            self.ensure_region(Region::Side)?;
        }
        let packet = lighting::wireless_frame(stamp, effect, side).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid wireless lighting mode",
            )
        })?;
        self.send(packet)
    }

    pub fn stream_wireless_lighting(&mut self, cancelled: impl Fn() -> bool) -> io::Result<()> {
        if self.read_link_status()?.transport != 1 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "Connect through a supported 2.4 GHz receiver to stream effects",
            ));
        }
        let effect = self.read_lighting(Region::Keyboard)?;
        if effect.mode == 100 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Start Audio-reactive lighting to stream audio",
            ));
        }
        let side = if self.layout.side_lighting {
            Some(self.read_lighting(Region::Side)?)
        } else {
            None
        };
        let mut frames = wireless::Live::new(self.layout, effect, side)?;
        let mut input = if frames.takes_input() {
            Some(key_input::Input::open(&self.interface)?)
        } else {
            None
        };
        if cancelled() {
            return Ok(());
        }
        self.with_lighting_sync(move |keyboard| {
            while !cancelled() {
                let started = Instant::now();
                if let Some(input) = &mut input
                    && let Some(key) = input.newest_press()?
                {
                    frames.press(keyboard.layout, key);
                }
                keyboard.send(frames.packet(lighting_stamp()?))?;
                thread::sleep(Duration::from_millis(32).saturating_sub(started.elapsed()));
            }
            Ok(())
        })
    }

    pub fn stream_usb_audio(
        &mut self,
        pcm: &mut impl Read,
        sample_rate: NonZeroU32,
        frequency_range: std::ops::Range<f64>,
        color: [u8; 3],
    ) -> io::Result<usize> {
        if self.read_link_status()?.transport != 0 {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "USB audio streaming requires a wired connection",
            ));
        }
        self.stream_audio_for(pcm, sample_rate, frequency_range, color, None)
    }

    pub fn stream_audio(
        &mut self,
        pcm: &mut impl Read,
        sample_rate: NonZeroU32,
        frequency_range: std::ops::Range<f64>,
        color: [u8; 3],
    ) -> io::Result<usize> {
        let wireless = self.audio_receiver()?;
        self.stream_audio_for(pcm, sample_rate, frequency_range, color, wireless)
    }

    fn audio_receiver(&mut self) -> io::Result<Option<wireless::Live>> {
        Ok(match self.read_link_status()?.transport {
            0 => None,
            1 => {
                let effect = self.read_lighting(Region::Keyboard)?;
                let side = if self.layout.side_lighting {
                    Some(self.read_lighting(Region::Side)?)
                } else {
                    None
                };
                Some(wireless::Live::new(
                    self.layout,
                    Effect {
                        mode: 100,
                        ..effect
                    },
                    side,
                )?)
            }
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Audio lighting needs a USB or 2.4 GHz connection",
                ));
            }
        })
    }

    pub fn stream_audio_bars(
        &mut self,
        input: &mut impl Read,
        palette: impl Fn() -> lighting::AudioPalette,
        ready: impl FnMut(),
    ) -> io::Result<()> {
        self.stream_audio_bars_guarded(input, palette, ready, || Ok(()))
    }

    /// Hold the configuration lock for HID reports and release it while awaiting audio.
    pub fn stream_audio_bars_guarded<G>(
        &mut self,
        input: &mut impl Read,
        palette: impl Fn() -> lighting::AudioPalette,
        mut ready: impl FnMut(),
        guard: impl Fn() -> io::Result<G>,
    ) -> io::Result<()> {
        let (mut wireless, setup) = {
            let _guard = guard()?;
            let wireless = self.audio_receiver()?;
            let setup = if wireless.is_none() {
                self.begin_wallpaper_lighting()
            } else {
                self.send(lighting::synchronize(true))
            };
            (wireless, setup)
        };
        let keys = lighting::AudioKeybed::new(self.layout);
        let result = setup.and_then(|()| {
            let mut bars = [0; BAR_COUNT];
            let mut previous = None;
            let mut last = Instant::now();
            let mut peaks = [0_u8; 11];
            let mut tick = 0_u8;
            while read_pcm_window(input, &mut bars)? {
                if bars.iter().any(|&height| height > 6) {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "Invalid audio bar height",
                    ));
                }
                let _guard = guard()?;
                if let Some(receiver) = &mut wireless {
                    let levels = std::array::from_fn(|i| bars[2 * i].max(bars[2 * i + 1]).min(5));
                    for (peak, level) in peaks.iter_mut().zip(levels) {
                        *peak = level.max(peak.saturating_sub(u8::from(tick.is_multiple_of(4))));
                    }
                    self.send(receiver.audio_packet(lighting_stamp()?, levels, peaks))?;
                    tick = tick.wrapping_add(1);
                } else {
                    let frame = keys.frame(&bars, palette());
                    let elapsed = last.elapsed();
                    if elapsed >= Duration::from_millis(500) {
                        self.keep_wallpaper_lighting()?;
                        last = Instant::now();
                    }
                    if previous != Some(frame) || elapsed >= Duration::from_secs(1) {
                        for packet in lighting::keyboard_frame(ColorTransfer::UsbStream, &frame) {
                            self.send(packet)?;
                        }
                        previous = Some(frame);
                    }
                }
                ready();
            }
            Ok(())
        });
        let cleanup = guard().and_then(|_guard| {
            if wireless.is_none() {
                self.stop_wallpaper_lighting()
            } else {
                self.handle.send(lighting::synchronize(false))
            }
        });
        result.and(cleanup)
    }

    fn stream_audio_for(
        &mut self,
        pcm: &mut impl Read,
        sample_rate: NonZeroU32,
        frequency_range: std::ops::Range<f64>,
        color: [u8; 3],
        mut wireless: Option<wireless::Live>,
    ) -> io::Result<usize> {
        if !frequency_range.start.is_finite()
            || !frequency_range.end.is_finite()
            || frequency_range.start < 0.0
            || frequency_range.start >= frequency_range.end
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid audio frequency range",
            ));
        }
        let mut spectrum = Spectrum::default();
        let mut levels = [0; 22];
        let mut peaks = [0; 11];
        let mut bytes = [0; WINDOW_SAMPLES * 2];
        let mut samples = [0; WINDOW_SAMPLES];
        self.with_lighting_sync(|keyboard| {
            let mut frames = 0_usize;
            let mut last = Instant::now();
            let frame_period =
                Duration::from_secs_f64(WINDOW_SAMPLES as f64 / f64::from(sample_rate.get()))
                    .max(Duration::from_millis(16));
            while read_pcm_window(pcm, &mut bytes)? {
                for (sample, pair) in samples.iter_mut().zip(bytes.chunks_exact(2)) {
                    *sample = i16::from_le_bytes([pair[0], pair[1]]);
                }
                spectrum.analyze(
                    &samples,
                    sample_rate,
                    frequency_range.start,
                    frequency_range.end,
                    &mut levels,
                );
                let delay = frame_period.saturating_sub(last.elapsed());
                if !delay.is_zero() {
                    thread::sleep(delay);
                }
                if let Some(wireless) = &mut wireless {
                    let bands = wireless_bands(&levels, &mut peaks, frames.is_multiple_of(4));
                    keyboard.send(wireless.audio_packet(lighting_stamp()?, bands, peaks))?;
                } else {
                    let frame = lighting::audio_frame(&levels, color);
                    for packet in lighting::keyboard_frame(ColorTransfer::UsbStream, &frame) {
                        keyboard.send(packet)?;
                    }
                }
                last = Instant::now();
                frames = frames.saturating_add(1);
            }
            Ok(frames)
        })
    }

    fn with_lighting_sync<T>(
        &mut self,
        stream: impl FnOnce(&mut Self) -> io::Result<T>,
    ) -> io::Result<T> {
        self.stop_wallpaper_lighting()?;
        self.send(lighting::synchronize(true))?;
        let result = stream(self);
        // Attempt the exit packet even if a failed send marked this session stale.
        let cleanup = self.handle.send(lighting::synchronize(false));
        if cleanup.is_err() {
            self.response_ready = false;
        }
        result.and_then(|value| cleanup.map(|()| value))
    }

    pub fn write_direct_mapping(&mut self, mapping: DirectMapping) -> io::Result<()> {
        self.send(mapping.packet())
    }

    pub fn has_desktop_bindings(&self) -> bool {
        self.response_ready && !self.desktop.is_empty()
    }

    pub fn poll_desktop_binding(&mut self) -> io::Result<Option<DesktopBinding>> {
        if !self.has_desktop_bindings() {
            return Ok(None);
        }
        let mut report = [0; 64];
        let id = self.handle.layout().output_id.byte();
        for _ in 0..16 {
            let count = self.handle.read_report(&mut report)?;
            if count == 0 {
                break;
            }
            if let Some(action) =
                desktop::pressed_key(&report[..count], id).and_then(|code| self.desktop.get(&code))
            {
                return Ok(Some(action.clone()));
            }
        }
        Ok(None)
    }

    pub fn discard_pending_bindings(&mut self) -> io::Result<()> {
        drain(&mut self.handle, &mut [0; 64])
    }

    pub fn clear_desktop_bindings(&mut self) {
        self.desktop.clear();
    }

    pub fn upload_profile(&mut self, plan: &ProfilePlan) -> io::Result<UploadResult> {
        self.ensure_ready()?;
        if plan.family != self.layout.family || !self.layout.onboard_profiles {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "profile plan belongs to a different model",
            ));
        }
        let report_id = self.handle.layout().output_id.byte();
        self.desktop.clear();
        let result = execute_profile(&mut self.handle, plan, report_id, thread::sleep);
        if matches!(result, Ok(UploadResult::DeviceAccepted)) {
            self.desktop.clone_from(&plan.desktop);
        }
        if result.is_err() {
            self.response_ready = false;
        }
        result
    }

    fn ensure_magnetic(&self) -> io::Result<()> {
        if self.layout.magnetic() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "keyboard has no Hall-effect settings",
            ))
        }
    }

    fn ensure_region(&self, region: Region) -> io::Result<()> {
        if region == Region::Side && !self.layout.side_lighting {
            return Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "keyboard has no side lighting",
            ));
        }
        Ok(())
    }

    fn send(&mut self, packet: Packet) -> io::Result<()> {
        self.ensure_ready()?;
        let result = self.handle.send(packet);
        if result.is_err() {
            self.response_ready = false;
        }
        result
    }

    fn query(&mut self, packet: Packet, reply: Reply) -> io::Result<Received> {
        self.ensure_ready()?;
        let report_id = self.handle.layout().output_id.byte();
        let result = exchange(&mut self.handle, packet, reply, report_id, REPLY_TIMEOUT);
        if result.is_err() {
            self.response_ready = false;
        }
        result
    }

    fn ensure_ready(&self) -> io::Result<()> {
        if self.response_ready {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "reopen the keyboard after a transport failure",
            ))
        }
    }
}

impl Drop for Keyboard {
    fn drop(&mut self) {
        let _ = self.stop_wallpaper_lighting();
    }
}

fn lighting_stamp() -> io::Result<u16> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|now| now.as_millis() as u16)
        .map_err(io::Error::other)
}

fn wireless_bands(levels: &[u8; 22], peaks: &mut [u8; 11], decay: bool) -> [u8; 11] {
    std::array::from_fn(|index| {
        let level = levels[2 * index + 1];
        peaks[index] = level.max(peaks[index].saturating_sub(u8::from(decay)));
        level
    })
}

fn read_pcm_window(reader: &mut impl Read, bytes: &mut [u8]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < bytes.len() {
        match reader.read(&mut bytes[filled..]) {
            Ok(0) if filled.is_multiple_of(2) => return Ok(false),
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "partial PCM sample",
                ));
            }
            Ok(count) => filled += count,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => continue,
            Err(error) => return Err(error),
        }
    }
    Ok(true)
}

trait Transport {
    fn send(&mut self, packet: Packet) -> io::Result<()>;
    fn wait(&mut self, timeout: Duration) -> io::Result<bool>;
    fn receive(&mut self, buffer: &mut Packet) -> io::Result<usize>;
}

impl Transport for HidrawDevice {
    fn send(&mut self, packet: Packet) -> io::Result<()> {
        let written = self.write_report(packet).map_err(|error| match error {
            WriteError::Io(error) => error,
            WriteError::InvalidFrame(frame) => io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid SDK frame {frame:?}"),
            ),
        })?;
        if written == usize::from(self.layout().output_len.get()) {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::WriteZero,
                "incomplete HID report",
            ))
        }
    }

    fn wait(&mut self, timeout: Duration) -> io::Result<bool> {
        self.wait_for_report(timeout)
    }

    fn receive(&mut self, buffer: &mut Packet) -> io::Result<usize> {
        self.read_report(buffer)
    }
}

#[derive(Debug)]
struct Received {
    report: Packet,
    length: usize,
}

impl AsRef<[u8]> for Received {
    fn as_ref(&self) -> &[u8] {
        &self.report[..self.length]
    }
}

fn exchange(
    io: &mut impl Transport,
    request: Packet,
    reply: Reply,
    report_id: u8,
    timeout: Duration,
) -> io::Result<Received> {
    let mut report = [0; 64];
    drain(io, &mut report)?;
    io.send(request)?;
    let started = Instant::now();
    loop {
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() || !io.wait(remaining)? {
            return Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "no matching keyboard reply",
            ));
        }
        let count = io.receive(&mut report)?;
        if reply.matches(&report[..count], report_id) {
            return Ok(Received {
                report,
                length: count,
            });
        }
    }
}

fn execute_profile(
    io: &mut impl Transport,
    plan: &ProfilePlan,
    report_id: u8,
    pause: impl Fn(Duration),
) -> io::Result<UploadResult> {
    let mut buffer = [0; 64];
    drain(io, &mut buffer)?;
    let mut status = UploadResult::Transmitted;
    for step in &plan.steps {
        match *step {
            UploadStep::Data(packet) => io.send(packet)?,
            UploadStep::Macro(packet) => {
                let mut acknowledged = false;
                for _ in 0..4 {
                    io.send(packet)?;
                    let started = Instant::now();
                    loop {
                        let remaining =
                            Duration::from_millis(100).saturating_sub(started.elapsed());
                        if remaining.is_zero() || !io.wait(remaining)? {
                            break;
                        }
                        let count = io.receive(&mut buffer)?;
                        let report = &buffer[..count];
                        observe_profile_status(report, report_id, &mut status)?;
                        if report.starts_with(&[report_id, 0x19]) {
                            acknowledged = true;
                            break;
                        }
                    }
                    if acknowledged {
                        break;
                    }
                }
                if !acknowledged {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "macro packet was not acknowledged; profile upload may be partial",
                    ));
                }
            }
            UploadStep::Pause => pause(Duration::from_millis(400)),
        }
    }
    let started = Instant::now();
    while status == UploadResult::Transmitted {
        let remaining = Duration::from_secs(3).saturating_sub(started.elapsed());
        if remaining.is_zero() || !io.wait(remaining)? {
            break;
        }
        let count = io.receive(&mut buffer)?;
        observe_profile_status(&buffer[..count], report_id, &mut status)?;
    }
    Ok(status)
}

fn observe_profile_status(
    report: &[u8],
    report_id: u8,
    status: &mut UploadResult,
) -> io::Result<()> {
    if report.starts_with(&[report_id, 1]) && report.get(5).is_some_and(|&length| length != 0) {
        match report.get(13).copied().unwrap_or(0) {
            0 => {}
            1 => *status = UploadResult::DeviceAccepted,
            _ => {
                return Err(io::Error::other(
                    "keyboard rejected profile upload; device state may be partial",
                ));
            }
        }
    }
    Ok(())
}

fn drain(io: &mut impl Transport, report: &mut Packet) -> io::Result<()> {
    for _ in 0..DRAIN_LIMIT {
        if io.receive(report)? == 0 {
            return Ok(());
        }
    }
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "keyboard input queue did not settle",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[derive(Default)]
    struct Mock {
        stale: VecDeque<Vec<u8>>,
        incoming: VecDeque<Vec<u8>>,
        sent: Vec<Packet>,
    }

    impl Transport for Mock {
        fn send(&mut self, packet: Packet) -> io::Result<()> {
            self.sent.push(packet);
            Ok(())
        }

        fn wait(&mut self, _: Duration) -> io::Result<bool> {
            Ok(!self.incoming.is_empty())
        }

        fn receive(&mut self, buffer: &mut Packet) -> io::Result<usize> {
            let queue = if self.sent.is_empty() {
                &mut self.stale
            } else {
                &mut self.incoming
            };
            let Some(report) = queue.pop_front() else {
                return Ok(0);
            };
            buffer[..report.len()].copy_from_slice(&report);
            Ok(report.len())
        }
    }

    #[test]
    fn query_discards_stale_and_unrelated_reports() {
        let control = Control::AntiShake;
        let mut mock = Mock {
            stale: VecDeque::from([vec![1, 0x21, 0, 0, 0, 0, 10, 0xff]]),
            incoming: VecDeque::from([
                vec![6, 0x21, 0, 0, 0, 0, 10, 0xee],
                vec![1, 0x21, 0, 0, 0, 0, 9, 0xcc],
                vec![1, 0x21, 0, 0, 0, 0, 10],
                vec![1, 0x21, 0, 0, 0, 0, 10, 9],
            ]),
            ..Mock::default()
        };
        let report = exchange(
            &mut mock,
            control.request(None),
            control.reply(),
            1,
            REPLY_TIMEOUT,
        )
        .unwrap();
        assert_eq!(report.as_ref(), [1, 0x21, 0, 0, 0, 0, 10, 9]);
        assert_eq!(mock.sent, [control.request(None)]);
    }

    #[test]
    fn query_times_out_after_a_single_write() {
        let mut mock = Mock::default();
        let control = Control::ReturnRate;
        let error = exchange(
            &mut mock,
            control.request(None),
            control.reply(),
            1,
            REPLY_TIMEOUT,
        )
        .unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(mock.sent.len(), 1);
    }

    #[test]
    fn upload_waits_for_matching_macro_ack_and_records_terminal_status() {
        let key_packet = crate::keyboard::transfer::onboard_chunk(0, 0, &[1, 2, 3, 4]).unwrap();
        let macro_packet = crate::keyboard::transfer::clear_macro_slot(0).unwrap()[0];
        let plan = ProfilePlan {
            family: "fixture",
            desktop: HashMap::new(),
            steps: vec![
                UploadStep::Data(key_packet),
                UploadStep::Pause,
                UploadStep::Macro(macro_packet),
            ],
        };
        let mut status = vec![0; 14];
        status[..2].copy_from_slice(&[1, 1]);
        status[5] = 8;
        status[13] = 1;
        let mut mock = Mock {
            incoming: VecDeque::from([vec![6, 0x19], vec![1, 0x18], vec![1, 0x19], status]),
            ..Mock::default()
        };
        assert_eq!(
            execute_profile(&mut mock, &plan, 1, |_| {}).unwrap(),
            UploadResult::DeviceAccepted
        );
        assert_eq!(mock.sent, [key_packet, macro_packet]);
    }

    #[test]
    fn upload_stops_after_four_unacknowledged_macro_writes() {
        let packet = crate::keyboard::transfer::clear_macro_slot(0).unwrap()[0];
        let plan = ProfilePlan {
            family: "fixture",
            desktop: HashMap::new(),
            steps: vec![UploadStep::Macro(packet), UploadStep::Data(packet)],
        };
        let mut mock = Mock::default();
        let error = execute_profile(&mut mock, &plan, 1, |_| {}).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::TimedOut);
        assert_eq!(mock.sent, [packet; 4]);
    }

    #[test]
    fn pcm_reader_reports_partial_samples_and_drops_an_incomplete_window() {
        let mut buffer = [0; 4];
        assert!(read_pcm_window(&mut &[1, 2, 3, 4][..], &mut buffer).unwrap());
        assert_eq!(buffer, [1, 2, 3, 4]);
        assert!(!read_pcm_window(&mut &[1, 2][..], &mut buffer).unwrap());
        assert!(read_pcm_window(&mut &[1][..], &mut buffer).is_err());
    }
}
