use std::cell::Cell;
use std::rc::Rc;

use dragon_driver::keyboard::models::{Layout, Position, geometry};
use dragon_driver::witmod_sdk::keymap::KeyMap;
use gtk::prelude::*;

pub(super) struct KeyGrid {
    pub area: gtk::Box,
    pub lighting_tools: gtk::Box,
    pub buttons: Rc<Vec<(Position, gtk::ToggleButton)>>,
}

impl KeyGrid {
    pub fn new(layout: &Layout) -> Self {
        let names = KeyMap::default();
        let keyboard = gtk::Fixed::new();
        keyboard.add_css_class("keybed");
        keyboard.set_halign(gtk::Align::Center);
        let geometry = geometry(layout);
        let left = geometry
            .iter()
            .map(|(_, [x, _, _, _])| *x)
            .min()
            .unwrap_or(0);
        let top = geometry
            .iter()
            .map(|(_, [_, y, _, _])| *y)
            .min()
            .unwrap_or(0);
        let right = geometry
            .iter()
            .map(|(_, [x, _, width, _])| x + width)
            .max()
            .unwrap_or(0);
        let scale = (690.0 / f64::from((right - left).max(1))).min(1.0);
        let scale_to_px = |value| (f64::from(value) * scale).round() as i32;
        let buttons: Vec<_> = geometry
            .into_iter()
            .filter_map(|(pos, [x, y, width, height])| {
                let code = layout.key(pos)?;
                let caption = caption(&names, code);
                let btn = gtk::ToggleButton::with_label(&caption);
                let name = accessible_name(&names, code);
                btn.set_tooltip_text(Some(&name));
                btn.update_property(&[gtk::accessible::Property::Label(&name)]);
                btn.add_css_class("keycap");
                btn.set_size_request(scale_to_px(width), scale_to_px(height));
                keyboard.put(
                    &btn,
                    f64::from(scale_to_px(x - left)),
                    f64::from(scale_to_px(y - top)),
                );
                Some((pos, btn))
            })
            .collect();
        drag_selection(&keyboard);
        let view = gtk::ScrolledWindow::builder()
            .hscrollbar_policy(gtk::PolicyType::Automatic)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .child(&keyboard)
            .build();
        view.set_hexpand(true);
        view.set_valign(gtk::Align::Center);
        let selection = gtk::Box::new(gtk::Orientation::Horizontal, 4);
        selection.set_halign(gtk::Align::End);
        selection.set_valign(gtk::Align::Center);
        let select = selection_button("edit-select-all-symbolic", "Select all");
        let clear = selection_button("edit-clear-symbolic", "Clear selection");
        selection.append(&select);
        selection.append(&clear);
        let balance = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        let sides = gtk::SizeGroup::new(gtk::SizeGroupMode::Horizontal);
        sides.add_widget(&balance);
        sides.add_widget(&selection);
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        row.append(&balance);
        row.append(&view);
        row.append(&selection);
        let lighting_tools = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        lighting_tools.set_halign(gtk::Align::End);
        let tools_pages = gtk::Stack::builder()
            .hhomogeneous(true)
            .vhomogeneous(true)
            .build();
        tools_pages.add_named(&gtk::Box::new(gtk::Orientation::Vertical, 0), Some("empty"));
        tools_pages.add_named(&lighting_tools, Some("lighting"));
        let area = gtk::Box::new(gtk::Orientation::Vertical, 4);
        area.append(&row);
        area.append(&tools_pages);
        area.connect_css_classes_notify(move |area| {
            let lighting = area.has_css_class("lighting-preview");
            tools_pages.set_visible_child_name(if lighting { "lighting" } else { "empty" });
        });
        let buttons = Rc::new(buttons);
        for (button, active) in [(select, true), (clear, false)] {
            let keys = buttons.clone();
            button.connect_clicked(move |_| {
                for (_, key) in keys.iter() {
                    key.set_active(active);
                }
            });
        }
        Self {
            area,
            lighting_tools,
            buttons,
        }
    }
}

fn selection_button(icon: &str, name: &str) -> gtk::Button {
    let button = gtk::Button::from_icon_name(icon);
    button.add_css_class("flat");
    button.set_tooltip_text(Some(name));
    button.update_property(&[gtk::accessible::Property::Label(name)]);
    button
}

#[derive(Clone, Copy)]
struct Drag {
    start: (f64, f64),
    previous: (f64, f64),
    active: bool,
    dragging: bool,
}

fn key_at(keyboard: &gtk::Fixed, x: f64, y: f64) -> Option<gtk::ToggleButton> {
    let mut widget = keyboard.pick(x, y, gtk::PickFlags::DEFAULT)?;
    loop {
        if let Some(key) = widget.downcast_ref::<gtk::ToggleButton>() {
            return Some(key.clone());
        }
        widget = widget.parent()?;
        if widget == *keyboard {
            return None;
        }
    }
}

fn select_segment(keyboard: &gtk::Fixed, from: (f64, f64), to: (f64, f64), active: bool) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let steps = (dx.abs().max(dy.abs()) / 5.0).ceil().clamp(1.0, 1024.0) as u32;
    for step in 0..=steps {
        let position = f64::from(step) / f64::from(steps);
        if let Some(key) = key_at(keyboard, from.0 + dx * position, from.1 + dy * position) {
            key.set_active(active);
        }
    }
}

fn drag_selection(keyboard: &gtk::Fixed) {
    let gesture = gtk::GestureDrag::builder()
        .button(1)
        .propagation_phase(gtk::PropagationPhase::Capture)
        .build();
    let state = Rc::new(Cell::new(None::<Drag>));
    let weak = keyboard.downgrade();
    let begin = state.clone();
    gesture.connect_drag_begin(move |_, x, y| {
        begin.set(
            weak.upgrade()
                .and_then(|keyboard| key_at(&keyboard, x, y))
                .map(|key| Drag {
                    start: (x, y),
                    previous: (x, y),
                    active: !key.is_active(),
                    dragging: false,
                }),
        );
    });
    let weak = keyboard.downgrade();
    let update = state.clone();
    gesture.connect_drag_update(move |gesture, dx, dy| {
        let (Some(keyboard), Some(mut drag)) = (weak.upgrade(), update.get()) else {
            return;
        };
        if !drag.dragging {
            if dx.hypot(dy) < 5.0 || !gesture.set_state(gtk::EventSequenceState::Claimed) {
                return;
            }
            drag.dragging = true;
        }
        let point = (drag.start.0 + dx, drag.start.1 + dy);
        select_segment(&keyboard, drag.previous, point, drag.active);
        drag.previous = point;
        update.set(Some(drag));
    });
    gesture.connect_drag_end(move |_, _, _| state.set(None));
    keyboard.add_controller(gesture);
}

pub(super) fn caption(names: &KeyMap, key: i16) -> String {
    match key {
        42 => "⌫".into(),
        57 => "Caps".into(),
        79 => "→".into(),
        80 => "←".into(),
        81 => "↓".into(),
        82 => "↑".into(),
        101 => "Menu".into(),
        224 | 228 => "Ctrl".into(),
        225 | 229 => "Shift".into(),
        226 | 230 => "Alt".into(),
        227 | 231 => "Super".into(),
        _ => names.label(i32::from(key)).unwrap_or("Key").to_owned(),
    }
}

pub(super) fn accessible_name(names: &KeyMap, code: i16) -> String {
    let name = match code {
        42 => "Backspace",
        79 => "Right arrow",
        80 => "Left arrow",
        81 => "Down arrow",
        82 => "Up arrow",
        224 => "Left Ctrl",
        225 => "Left Shift",
        226 => "Left Alt",
        227 => "Left Super",
        228 => "Right Ctrl",
        229 => "Right Shift",
        230 => "Right Alt",
        231 => "Right Super",
        _ => return caption(names, code),
    };
    name.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_visible_keys_match_vendor_matrix_coordinates() {
        for layout in dragon_driver::keyboard::models::layouts() {
            let geometry = geometry(layout);
            let positions: Vec<_> = geometry.iter().map(|(position, _)| *position).collect();
            assert_eq!(
                positions,
                layout
                    .keys()
                    .map(|(position, _)| position)
                    .collect::<Vec<_>>(),
                "{}",
                layout.family
            );
            assert!(
                geometry
                    .iter()
                    .all(|(_, [_, _, width, height])| *width > 0 && *height > 0)
            );
        }
    }
}
