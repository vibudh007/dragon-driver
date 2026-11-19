use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::time::Instant;

use gtk::{gdk, glib, prelude::*};

const LIMIT: usize = 62;

#[derive(Default)]
struct Recording {
    started: Option<Instant>,
    held: BTreeMap<u32, u8>,
    events: Vec<String>,
}

impl Recording {
    fn event(&mut self, usage: u8, down: bool) {
        let now = Instant::now();
        let delay = self.started.map_or(0, |last| {
            now.duration_since(last)
                .as_millis()
                .min(u128::from(u16::MAX))
        });
        self.events.push(format!(
            "{delay} key {usage} {}",
            if down { "down" } else { "up" }
        ));
        self.started = Some(now);
    }

    fn release_all(&mut self) {
        for (_, usage) in std::mem::take(&mut self.held) {
            self.event(usage, false);
        }
    }
}

pub(super) fn attach(view: &gtk::TextView, record: &gtk::ToggleButton) {
    let recording = Rc::new(RefCell::new(Recording::default()));
    let copy = recording.clone();
    let weak_view = view.downgrade();
    record.connect_toggled(move |record| {
        let Some(view) = weak_view.upgrade() else {
            return;
        };
        if record.is_active() {
            *copy.borrow_mut() = Recording::default();
            view.buffer().set_text("");
            record.set_label("Stop recording");
            view.grab_focus();
        } else {
            let mut recording = copy.borrow_mut();
            recording.release_all();
            view.buffer().set_text(&recording.events.join("\n"));
            record.set_label("Record replacement");
        }
        view.set_editable(!record.is_active());
    });

    let keys = gtk::EventControllerKey::new();
    let weak_record = record.downgrade();
    let weak_view = view.downgrade();
    let pressed = recording.clone();
    keys.connect_key_pressed(move |_, key, physical, state| {
        let Some(record) = weak_record.upgrade().filter(|record| record.is_active()) else {
            return glib::Propagation::Proceed;
        };
        if key == gdk::Key::Escape && state.contains(gdk::ModifierType::CONTROL_MASK) {
            record.set_active(false);
        } else if let Some(usage) = key_usage(key) {
            let mut recording = pressed.borrow_mut();
            if !recording.held.contains_key(&physical)
                && recording.events.len() + recording.held.len() + 2 <= LIMIT
            {
                recording.held.insert(physical, usage);
                recording.event(usage, true);
                if let Some(view) = weak_view.upgrade() {
                    view.buffer().set_text(&recording.events.join("\n"));
                }
            }
        }
        glib::Propagation::Stop
    });
    let weak_record = record.downgrade();
    let weak_view = view.downgrade();
    keys.connect_key_released(move |_, _, physical, _| {
        if !weak_record
            .upgrade()
            .is_some_and(|record| record.is_active())
        {
            return;
        }
        let mut recording = recording.borrow_mut();
        if let Some(usage) = recording.held.remove(&physical) {
            recording.event(usage, false);
            if let Some(view) = weak_view.upgrade() {
                view.buffer().set_text(&recording.events.join("\n"));
            }
        }
    });
    view.add_controller(keys);
}

fn key_usage(key: gdk::Key) -> Option<u8> {
    match key {
        gdk::Key::Return => return Some(40),
        gdk::Key::Escape => return Some(41),
        gdk::Key::BackSpace => return Some(42),
        gdk::Key::Tab | gdk::Key::ISO_Left_Tab => return Some(43),
        gdk::Key::Caps_Lock => return Some(57),
        gdk::Key::Print => return Some(70),
        gdk::Key::Scroll_Lock => return Some(71),
        gdk::Key::Pause => return Some(72),
        gdk::Key::Insert => return Some(73),
        gdk::Key::Home => return Some(74),
        gdk::Key::Page_Up => return Some(75),
        gdk::Key::Delete => return Some(76),
        gdk::Key::End => return Some(77),
        gdk::Key::Page_Down => return Some(78),
        gdk::Key::Right => return Some(79),
        gdk::Key::Left => return Some(80),
        gdk::Key::Down => return Some(81),
        gdk::Key::Up => return Some(82),
        gdk::Key::Control_L => return Some(224),
        gdk::Key::Shift_L => return Some(225),
        gdk::Key::Alt_L => return Some(226),
        gdk::Key::Super_L => return Some(227),
        gdk::Key::Control_R => return Some(228),
        gdk::Key::Shift_R => return Some(229),
        gdk::Key::Alt_R | gdk::Key::ISO_Level3_Shift => return Some(230),
        gdk::Key::Super_R => return Some(231),
        _ => (),
    }
    if let Some(name) = key.name()
        && let Some(function) = name
            .strip_prefix('F')
            .and_then(|number| number.parse::<u8>().ok())
        && (1..=12).contains(&function)
    {
        return Some(57 + function);
    }
    let c = key.to_unicode()?.to_ascii_lowercase();
    let usage = match c {
        'a'..='z' => 4 + c as u8 - b'a',
        '1'..='9' => 30 + c as u8 - b'1',
        '0' => 39,
        ' ' => 44,
        '-' | '_' => 45,
        '=' | '+' => 46,
        '[' | '{' => 47,
        ']' | '}' => 48,
        '\\' | '|' => 49,
        ';' | ':' => 51,
        '\'' | '"' => 52,
        '`' | '~' => 53,
        ',' | '<' => 54,
        '.' | '>' => 55,
        '/' | '?' => 56,
        '!' => 30,
        '@' => 31,
        '#' => 32,
        '$' => 33,
        '%' => 34,
        '^' => 35,
        '&' => 36,
        '*' => 37,
        '(' => 38,
        ')' => 39,
        _ => return None,
    };
    Some(usage)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_shifted_keys_and_releases_held_keys() {
        assert_eq!(key_usage(gdk::Key::W), Some(26));
        assert_eq!(key_usage(gdk::Key::exclam), Some(30));
        assert_eq!(key_usage(gdk::Key::Control_R), Some(228));
        assert_eq!(key_usage(gdk::Key::F12), Some(69));
        let mut recording = Recording::default();
        recording.held.insert(30, 4);
        recording.event(4, true);
        recording.release_all();
        assert_eq!(recording.events.first().unwrap(), "0 key 4 down");
        assert!(recording.events[1].ends_with("key 4 up"));
        assert!(recording.held.is_empty());
    }
}
