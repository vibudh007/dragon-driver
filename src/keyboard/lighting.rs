use crate::witmod_sdk::keymap::{KEY_COLUMNS, KEY_ROWS};

use super::report::{Packet, Reply, packet};

#[cfg_attr(feature = "daemon", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Region {
    Keyboard,
    Side,
}

#[cfg_attr(feature = "daemon", derive(serde::Deserialize, serde::Serialize))]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Effect {
    pub mode: u8,
    pub brightness: u16,
    pub speed: u16,
    pub foreground: [u8; 3],
    pub background: [u8; 3],
    pub direction: u8,
    pub effect_type: u8,
    pub open: u8,
}

#[derive(Clone, Eq, PartialEq)]
pub struct SupportedModes {
    pub modes: Vec<u8>,
    pub settings: [u8; 2],
}

impl Region {
    pub fn index(self) -> usize {
        match self {
            Self::Keyboard => 0,
            Self::Side => 1,
        }
    }

    fn command(self) -> u8 {
        match self {
            Self::Keyboard => 6,
            Self::Side => 7,
        }
    }

    pub fn query(self) -> Packet {
        packet(self.command(), [1, 0, 0, 0])
    }

    pub fn modes(self) -> Packet {
        packet(self.mode_reply().command - 1, [0; 4])
    }

    pub(crate) fn mode_reply(self) -> Reply {
        Reply {
            command: match self {
                Self::Keyboard => 10,
                Self::Side => 2,
            },
            selector: None,
            min_len: 9,
        }
    }

    pub fn decode_modes(self, report: &[u8]) -> Option<SupportedModes> {
        if report.get(1) != Some(&self.mode_reply().command) {
            return None;
        }
        let mut body = report
            .get(6..)?
            .splitn(2, |&byte| matches!(byte, 0xa5 | 0x5a));
        let modes = body.next()?;
        let mut extra = body
            .next()?
            .iter()
            .copied()
            .skip_while(|byte| matches!(byte, 0xa5 | 0x5a));
        let first = extra.next()?;
        let second = extra.next()?;
        Some(SupportedModes {
            modes: modes.to_vec(),
            settings: [first, second],
        })
    }

    pub fn effect(self, effect: Effect, change_stamp: u16) -> Option<Packet> {
        let brightness = u8::try_from(effect.brightness.checked_sub(1)?).ok()?;
        let speed = u8::try_from(effect.speed.checked_sub(1)?).ok()?;
        let length = if self == Self::Keyboard { 14 } else { 13 };
        let mut report = packet(self.command(), [0, 0, 0, length]);
        report[6..9].copy_from_slice(&[effect.mode, brightness, speed]);
        report[9..12].copy_from_slice(&effect.foreground);
        report[12..15].copy_from_slice(&effect.background);
        report[15] = effect.direction;
        report[16] = if effect.mode == 0 {
            0
        } else {
            effect.effect_type
        };
        report[17] = effect.open;
        report[62..64].copy_from_slice(&change_stamp.to_be_bytes());
        Some(report)
    }

    pub fn decode(self, report: &[u8]) -> Option<Effect> {
        if report.get(1) != Some(&(self.command() + 1)) {
            return None;
        }
        let &[
            mode,
            brightness,
            speed,
            red,
            green,
            blue,
            bg_red,
            bg_green,
            bg_blue,
            direction,
            effect_type,
            open,
        ] = report.get(6..18)?
        else {
            return None;
        };
        Some(Effect {
            mode,
            brightness: u16::from(brightness) + 1,
            speed: u16::from(speed) + 1,
            foreground: [red, green, blue],
            background: [bg_red, bg_green, bg_blue],
            direction,
            effect_type,
            open,
        })
    }

    pub(crate) fn reply(self) -> Reply {
        Reply {
            command: self.command() + 1,
            selector: None,
            min_len: 18,
        }
    }
}

pub fn synchronize(enabled: bool) -> Packet {
    let mut report = packet(0x16, [0, 0, 0, 1]);
    report[6] = u8::from(enabled);
    report
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ColorTransfer {
    Custom,
    UsbStream,
}

pub fn color_chunk(transfer: ColorTransfer, page: u8, index: u8, payload: &[u8]) -> Option<Packet> {
    if payload.is_empty() || payload.len() > 58 {
        return None;
    }
    let mut report = packet(
        match transfer {
            ColorTransfer::Custom => 8,
            ColorTransfer::UsbStream => 0x0e,
        },
        [page, 0, index, payload.len() as u8],
    );
    report[6..6 + payload.len()].copy_from_slice(payload);
    Some(report)
}

pub type RgbFrame = [[[u8; 3]; KEY_COLUMNS]; KEY_ROWS];
pub type AudioPalette = [[u8; 3]; 4];
pub const DEFAULT_AUDIO_PALETTE: AudioPalette =
    [[1, 1, 4], [24, 76, 255], [114, 48, 255], [255, 70, 32]];

pub(crate) struct AudioKeybed {
    positions: Vec<(usize, usize, std::ops::Range<usize>, u8)>,
}

impl AudioKeybed {
    pub(crate) fn new(layout: &super::models::Layout) -> Self {
        let mut top = KEY_ROWS;
        let mut bottom = 0;
        let mut left = KEY_COLUMNS;
        let mut right = 0;
        for (key, _) in layout.keys() {
            let (row, column) = (usize::from(key.row()), usize::from(key.column()));
            top = top.min(row);
            bottom = bottom.max(row);
            left = left.min(column);
            right = right.max(column);
        }
        let positions = layout
            .keys()
            .map(|(key, _)| {
                let (row, column) = (usize::from(key.row()), usize::from(key.column()));
                let width = right - left + 1;
                let start = (column - left) * super::audio::BAR_COUNT / width;
                let end = (column - left + 1) * super::audio::BAR_COUNT / width;
                let height = ((bottom - row) * 6 / (bottom - top + 1) + 1) as u8;
                (row, column, start..end, height)
            })
            .collect();
        Self { positions }
    }

    pub(crate) fn frame(
        &self,
        bars: &[u8; super::audio::BAR_COUNT],
        palette: AudioPalette,
    ) -> RgbFrame {
        let mut frame = RgbFrame::default();
        for (row, column, band, height) in &self.positions {
            let level = bars[band.clone()].iter().copied().max().unwrap_or(0);
            let color = if level < *height {
                0
            } else {
                usize::from((height - 1) / 2 + 1)
            };
            frame[*row][*column] = palette[color];
        }
        frame
    }
}

pub enum WirelessEffect {
    Color { mode: u8, color: [u8; 3] },
    Automatic { mode: u8 },
    KeyEvent { mode: u8, event: Option<[u8; 5]> },
    RowEvent(Option<[u8; 2]>),
    AutoRipple(Option<[u8; 5]>),
    Audio { levels: [u8; 11], peaks: [u8; 11] },
}

pub enum WirelessSide {
    Color { mode: u8, color: [u8; 3] },
    Wave,
    Collision([u8; 5]),
}

pub fn wireless_frame(
    stamp: u16,
    keyboard: WirelessEffect,
    side: Option<WirelessSide>,
) -> Option<Packet> {
    let [high, low] = stamp.to_be_bytes();
    let mut report = packet(0x28, [high, low, 0, 0]);
    report[4] = match keyboard {
        WirelessEffect::Color {
            mode: mode @ (0 | 1 | 3),
            color,
        } => {
            report[5..8].copy_from_slice(&color);
            mode
        }
        WirelessEffect::Automatic {
            mode: mode @ (2 | 4 | 5 | 9 | 15),
        } => mode,
        WirelessEffect::KeyEvent {
            mode: mode @ (6 | 7 | 8 | 11 | 16),
            event,
        } => {
            report[5..10].copy_from_slice(&event.unwrap_or_default());
            mode
        }
        WirelessEffect::RowEvent(event) => {
            report[5..7].copy_from_slice(&event.unwrap_or_default());
            12
        }
        WirelessEffect::AutoRipple(event) => {
            report[5..10].copy_from_slice(&event.unwrap_or_default());
            14
        }
        WirelessEffect::Audio { levels, peaks } => {
            report[5..16].copy_from_slice(&levels);
            report[16..27].copy_from_slice(&peaks);
            100
        }
        _ => return None,
    };
    if let Some(side) = side {
        report[34] = match side {
            WirelessSide::Color {
                mode: mode @ (0 | 1 | 4 | 5),
                color,
            } => {
                report[35..38].copy_from_slice(&color);
                mode
            }
            WirelessSide::Wave => 2,
            WirelessSide::Collision(state) => {
                report[35..40].copy_from_slice(&state);
                3
            }
            _ => return None,
        };
    }
    Some(report)
}

pub fn keyboard_frame(transfer: ColorTransfer, frame: &RgbFrame) -> [Packet; 8] {
    let page = match transfer {
        ColorTransfer::Custom => 2,
        ColorTransfer::UsbStream => 0,
    };
    rgb_frame(transfer, page, frame)
}

pub(crate) fn rgb_frame(transfer: ColorTransfer, page: u8, frame: &RgbFrame) -> [Packet; 8] {
    let mut cells = frame.iter().flatten();
    std::array::from_fn(|index| {
        let mut data = [0; 54];
        let count = if index == 7 { 6 } else { 18 };
        for (color, bytes) in cells.by_ref().take(count).zip(data.chunks_exact_mut(3)) {
            bytes.copy_from_slice(color);
        }
        color_chunk(transfer, page, index as u8, &data[..count * 3]).expect("valid RGB chunk")
    })
}

pub(crate) fn activate_custom(mode: u8, brightness: u16, speed: u16) -> Option<Packet> {
    let mut report = packet(6, [0, 0, 0, 14]);
    report[6..9].copy_from_slice(&[
        mode,
        u8::try_from(brightness.checked_sub(1)?).ok()?,
        u8::try_from(speed.checked_sub(1)?).ok()?,
    ]);
    Some(report)
}

pub(crate) fn audio_frame(levels: &[u8; KEY_COLUMNS], color: [u8; 3]) -> RgbFrame {
    std::array::from_fn(|row| {
        std::array::from_fn(|column| {
            let height = usize::from(levels[column]);
            if height != 0 && height >= KEY_ROWS - 1 - row {
                color
            } else {
                [0; 3]
            }
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_bars_grow_upward_through_four_colors() {
        let layout = super::super::models::find("7153_AUSHERGB").unwrap();
        let keys = AudioKeybed::new(layout);
        let palette = DEFAULT_AUDIO_PALETTE;
        let full = keys.frame(&[6; super::super::audio::BAR_COUNT], palette);
        assert_eq!(full[1][0], palette[3]);
        assert_eq!(full[5][0], palette[1]);
        assert_eq!(full[0][0], [0; 3]);
        let silent = keys.frame(&[0; super::super::audio::BAR_COUNT], palette);
        assert_eq!(silent[1][0], palette[0]);
        assert_eq!(silent[5][0], palette[0]);
    }

    #[test]
    fn wireless_audio_and_collision_use_disjoint_wire_fields() {
        let packet = wireless_frame(
            0x1234,
            WirelessEffect::Audio {
                levels: [2; 11],
                peaks: [3; 11],
            },
            Some(WirelessSide::Collision([4, 5, 6, 7, 8])),
        )
        .unwrap();
        assert_eq!(packet[..5], [1, 0x28, 0x12, 0x34, 100]);
        assert_eq!(packet[5..16], [2; 11]);
        assert_eq!(packet[16..27], [3; 11]);
        assert_eq!(packet[34..40], [3, 4, 5, 6, 7, 8]);
    }

    #[test]
    fn frames_follow_zero_based_vendor_chunking() {
        let mut frame = [[[0; 3]; 22]; 6];
        frame[0][0] = [1, 2, 3];
        frame[5][21] = [4, 5, 6];
        let packets = keyboard_frame(ColorTransfer::UsbStream, &frame);
        assert_eq!(&packets[0][..9], &[1, 0x0e, 0, 0, 0, 54, 1, 2, 3]);
        assert_eq!(&packets[7][..6], &[1, 0x0e, 0, 0, 7, 18]);
        assert_eq!(&packets[7][21..24], &[4, 5, 6]);
        assert_eq!(
            keyboard_frame(ColorTransfer::Custom, &frame)[0][..3],
            [1, 8, 2]
        );
        assert_eq!(
            rgb_frame(ColorTransfer::Custom, 1, &frame)[0][..3],
            [1, 8, 1]
        );
        let activate = activate_custom(6, 4, 3).unwrap();
        assert_eq!(activate[..9], [1, 6, 0, 0, 0, 14, 6, 3, 2]);
        assert!(activate[9..].iter().all(|&byte| byte == 0));
    }

    #[test]
    fn mode_list_keeps_order_and_detects_truncation() {
        let modes = [2, 1, 3, 9, 4, 0, 14, 15, 6, 7, 11, 12, 16, 8, 10];
        let mut report = vec![1, 10, 0, 0, 0, 0];
        report.extend(modes);
        report.extend([0xa5, 0x5a, 4, 4]);
        let decoded = Region::Keyboard.decode_modes(&report).unwrap();
        assert_eq!(decoded.modes, modes);
        assert_eq!(decoded.settings, [4, 4]);
        assert!(
            Region::Keyboard
                .decode_modes(&report[..report.len() - 1])
                .is_none()
        );
        assert!(Region::Side.decode_modes(&report).is_none());
    }

    #[test]
    fn reads_and_writes_lighting_level_and_timestamp_fields() {
        let effect = Effect {
            mode: 3,
            brightness: 4,
            speed: 2,
            foreground: [1, 2, 3],
            background: [4, 5, 6],
            direction: 7,
            effect_type: 8,
            open: 9,
        };
        let mut packet = Region::Keyboard.effect(effect, 0x1234).unwrap();
        assert_eq!(
            &packet[..18],
            &[1, 6, 0, 0, 0, 14, 3, 3, 1, 1, 2, 3, 4, 5, 6, 7, 8, 9]
        );
        assert_eq!(&packet[62..], &[0x12, 0x34]);
        packet[1] = 7;
        assert_eq!(Region::Keyboard.decode(&packet[..18]), Some(effect));
        assert_eq!(Region::Keyboard.decode(&packet[..17]), None);
    }
}
