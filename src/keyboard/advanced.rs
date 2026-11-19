use std::io;

use super::bindings::combination;
use super::report::Packet;
use super::transfer::advanced_slot;
use crate::witmod_sdk::profile::AdvancedAction;

pub fn encode(slot: u8, action: AdvancedAction<'_>) -> io::Result<Packet> {
    let mut body = [0; 50];
    match action {
        AdvancedAction::Dks {
            key_codes,
            stages,
            values,
        } => {
            body[0] = 1;
            for (dst, value) in body[1..5].iter_mut().zip(values) {
                *dst = byte(value)?;
            }
            for (key, code) in key_codes.into_iter().enumerate() {
                let start = 5 + key * 8;
                body[start..start + 4].copy_from_slice(&action_key(code)?);
                for (stage, dst) in body[start + 4..start + 8].iter_mut().enumerate() {
                    *dst = byte(stages[stage][key])?;
                }
            }
        }
        AdvancedAction::ModTap {
            key_codes,
            value_time,
        } => {
            if value_time < 0 {
                return Err(invalid());
            }
            body[0] = 2;
            body[1..5].copy_from_slice(&action_key(key_codes[0])?);
            body[5..9].copy_from_slice(&action_key(key_codes[1])?);
            body[9] = byte(value_time / 10)?;
        }
        AdvancedAction::Toggle { key_code, .. } => {
            body[0] = 3;
            body[1..5].copy_from_slice(&action_key(key_code)?);
        }
    }
    advanced_slot(slot, 0, body).ok_or_else(invalid)
}

fn action_key(code: &str) -> io::Result<[u8; 4]> {
    let code = code.trim();
    if code.contains(',') {
        return combination(code);
    }
    if code.is_empty() {
        return Ok([0; 4]);
    }
    let value = code.parse::<i32>().map_err(|_| invalid())?;
    let key = if value < 0 {
        i8::try_from(value).map_err(|_| invalid())? as u8
    } else {
        byte(value)?
    };
    Ok([0, key, 0, 0])
}

fn byte(value: i32) -> io::Result<u8> {
    u8::try_from(value).map_err(|_| invalid())
}

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid advanced-key slot or value",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dks_interleaves_stages_for_each_action_and_keeps_stroke_order() {
        let packet = encode(
            3,
            AdvancedAction::Dks {
                key_codes: ["4", "3,41", "6", ""],
                stages: [
                    [1, 2, 3, 4],
                    [5, 6, 7, 8],
                    [9, 10, 11, 12],
                    [13, 14, 15, 16],
                ],
                values: [15, 36, 37, 14],
            },
        )
        .unwrap();
        assert_eq!(&packet[..11], &[1, 0x2a, 3, 0, 0, 50, 1, 15, 36, 37, 14]);
        assert_eq!(
            &packet[11..27],
            &[0, 4, 0, 0, 1, 5, 9, 13, 0x13, 0xe0, 0xe1, 41, 2, 6, 10, 14]
        );
        assert_eq!(&packet[31..43], &[3, 7, 11, 15, 0, 0, 0, 0, 4, 8, 12, 16]);
        assert!(packet[43..].iter().all(|&byte| byte == 0));
    }

    #[test]
    fn mod_tap_has_a_ten_millisecond_time_unit_and_toggle_has_one_action() {
        let mt = encode(
            0,
            AdvancedAction::ModTap {
                key_codes: ["4", "1,6"],
                value_time: 199,
            },
        )
        .unwrap();
        assert_eq!(&mt[6..16], &[2, 0, 4, 0, 0, 0x13, 0xe0, 6, 0, 19]);
        let toggle = encode(
            39,
            AdvancedAction::Toggle {
                key_code: "41",
                status: 1,
            },
        )
        .unwrap();
        assert_eq!(&toggle[6..11], &[3, 0, 41, 0, 0]);
        assert!(toggle[11..].iter().all(|&byte| byte == 0));
    }
}
