use std::collections::TryReserveError;

use super::profile::{MacroEvent, decimal};

pub const MIN_ONBOARD_WORDS: usize = 248;

pub fn encode_onboard_events(events: &[MacroEvent<'_>]) -> Result<Vec<u32>, TryReserveError> {
    let mut words = Vec::new();
    words.try_reserve(MIN_ONBOARD_WORDS)?;
    let mut encodable = events
        .iter()
        .filter_map(|event| event_code(event).map(|code| (event, code)))
        .peekable();
    while let Some((event, code)) = encodable.next() {
        words.try_reserve(4)?;
        let mouse = u32::from(event.event_type == "mouse");
        let down = u32::from(event.explanation == "down");
        words.extend([code, (down << 4) | mouse]);
        let delay = encodable.peek().map_or(0, |(next, _)| decimal(next.delay));
        if delay < 256 {
            words.extend([0, delay as u32]);
        } else {
            let delay = delay as u32;
            words.extend([(delay >> 8) & 0xff, delay & 0xff]);
        }
    }
    words.resize(words.len().max(MIN_ONBOARD_WORDS), 0);
    Ok(words)
}

pub(crate) fn event_code(event: &MacroEvent<'_>) -> Option<u32> {
    match event.event_type {
        "key" => Some(decimal(event.key_code) as u32),
        "mouse" => match event.key_code {
            "513" | "514" => Some(1),
            "516" | "517" => Some(2),
            "519" | "520" => Some(4),
            _ => None,
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reproduces_vendor_event_words_and_uses_the_next_delay() {
        let key = MacroEvent {
            mark: "0",
            delay: "9",
            event_type: "key",
            key_code: "41",
            code_name: "Esc",
            explanation: "down",
        };
        let press = MacroEvent {
            delay: "258",
            event_type: "mouse",
            key_code: "519",
            ..key
        };
        let release = MacroEvent {
            delay: "20",
            key_code: "520",
            explanation: "up",
            ..press
        };
        let output = encode_onboard_events(&[key, press, release]).unwrap();
        assert_eq!(&output[..12], &[41, 16, 1, 2, 4, 17, 0, 20, 4, 1, 0, 0]);
        assert_eq!(output.len(), 248);
        assert!(output[12..].iter().all(|&word| word == 0));
        assert_eq!(encode_onboard_events(&[key; 63]).unwrap().len(), 252);
    }

    #[test]
    fn skips_unsupported_events_without_shifting_subsequent_records() {
        let key = MacroEvent {
            mark: "0",
            delay: "0",
            event_type: "key",
            key_code: "41",
            code_name: "Esc",
            explanation: "down",
        };
        let wheel = MacroEvent {
            event_type: "mouse",
            key_code: "522",
            delay: "10",
            ..key
        };
        let release = MacroEvent {
            delay: "31",
            explanation: "up",
            ..key
        };
        let words = encode_onboard_events(&[key, wheel, release]).unwrap();
        assert_eq!(&words[..8], &[41, 16, 0, 31, 41, 0, 0, 0]);
        assert!(words[8..].iter().all(|&word| word == 0));
    }
}
