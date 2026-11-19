use std::cell::Cell;
use std::io;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU16, Ordering};
use std::time::{Duration, Instant};

use dragon_driver::keyboard::{
    hall::{CalibrationStatus, Control, Readback, Thresholds, Tuning},
    models::{Layout, Position},
    performance::{Sleep, WinLock},
};
use gtk::{Orientation, glib, prelude::*};

use super::keys::KeyGrid;
use super::readbacks::{KeyState, Readbacks};
use super::worker::{Connected, Tasks};
use super::{confirm, field, label, page, panel, primary};

mod changes;
mod lights;
mod reactive;
pub(super) use lights::lighting;
#[cfg(test)]
pub(super) use reactive::sync_switch;

pub(super) fn actuation(grid: &KeyGrid, tasks: &Rc<Tasks>, readbacks: &Rc<Readbacks>) -> gtk::Box {
    let page = page("Actuation", "preferences-system-symbolic");
    let selection = gtk::Box::new(Orientation::Vertical, 6);
    selection.add_css_class("selection-bar");
    let buttons = grid.buttons.clone();
    let count = label("No keys selected", "muted");
    let live = gtk::Button::with_label("Track travel");
    live.add_css_class("travel-action");
    live.set_tooltip_text(Some("Show live readings for the selected key"));
    live.set_sensitive(false);
    let bar = gtk::Box::new(Orientation::Horizontal, 8);
    count.set_hexpand(true);
    bar.append(&count);
    bar.append(&live);
    selection.append(&bar);
    let travel = label("", "muted");
    travel.set_visible(false);
    selection.append(&travel);
    page.append(&selection);

    let form = panel("Trigger settings");
    let mode = gtk::CheckButton::with_label("Separate press and release");
    let rapid = gtk::CheckButton::with_label("Rapid Trigger");
    let input: [gtk::Scale; 4] = std::array::from_fn(|_| distance());
    input[0].set_tooltip_text(Some(
        "Depth measured from the resting position for the first keypress",
    ));
    rapid.set_tooltip_text(Some(
        "Releases and retriggers after the key reverses direction by the chosen distance",
    ));
    let release = field("Release · mm", &input[1]);
    let rapid_press = field("Rapid press · mm", &input[2]);
    let rapid_release = field("Rapid release · mm", &input[3]);
    let apply = primary("Apply to selected keys");
    apply.set_sensitive(false);
    for widget in [
        mode.upcast_ref::<gtk::Widget>(),
        field("Press · mm", &input[0]).upcast_ref(),
        release.upcast_ref(),
        rapid.upcast_ref(),
        rapid_press.upcast_ref(),
        rapid_release.upcast_ref(),
    ] {
        form.append(widget);
    }
    mode.bind_property("active", &release, "visible")
        .sync_create()
        .build();
    rapid
        .bind_property("active", &rapid_press, "visible")
        .sync_create()
        .build();
    let update = {
        let row = rapid_release.downgrade();
        let mode = mode.downgrade();
        let rapid = rapid.downgrade();
        move || {
            if let (Some(row), Some(mode), Some(rapid)) =
                (row.upgrade(), mode.upgrade(), rapid.upgrade())
            {
                row.set_visible(mode.is_active() && rapid.is_active());
            }
        }
    };
    update();
    let copy = update.clone();
    mode.connect_toggled(move |_| copy());
    rapid.connect_toggled(move |_| update());

    let feedback = label("Select a key to see its saved settings.", "muted");
    feedback.set_wrap(true);
    let footer = gtk::Box::new(Orientation::Horizontal, 12);
    feedback.set_hexpand(true);
    footer.append(&feedback);
    footer.append(&apply);
    footer.add_css_class("panel-footer");
    form.append(&gtk::Separator::new(Orientation::Horizontal));
    form.append(&footer);
    page.append(&form);

    wire_travel(
        &live,
        &buttons,
        &travel,
        tasks,
        [
            &grid.area.clone().upcast::<gtk::Widget>(),
            form.upcast_ref(),
        ],
    );
    let limit = Rc::new(Cell::new(None::<u8>));
    let displayed = Rc::new(Cell::new(None::<Readback>));
    let weak_buttons = Rc::downgrade(&buttons);
    let update = {
        let (apply, count, live, feedback) = (
            apply.downgrade(),
            count.downgrade(),
            live.downgrade(),
            feedback.downgrade(),
        );
        let readbacks = Rc::downgrade(readbacks);
        let limit = limit.clone();
        let displayed = displayed.clone();
        move || {
            let (
                Some(buttons),
                Some(apply),
                Some(count),
                Some(live),
                Some(feedback),
                Some(readbacks),
            ) = (
                weak_buttons.upgrade(),
                apply.upgrade(),
                count.upgrade(),
                live.upgrade(),
                feedback.upgrade(),
                readbacks.upgrade(),
            )
            else {
                return;
            };
            let chosen = selected(&buttons);
            displayed.set(None);
            live.set_sensitive(chosen.len() == 1);
            apply.set_sensitive(limit.get().is_some() && chosen.len() > 1);
            count.set_label(&match chosen.len() {
                0 => "No keys selected".into(),
                1 => "1 key selected".into(),
                n => format!("{n} keys selected"),
            });
            match chosen.as_slice() {
                [key] => readbacks.select(Some(*key)),
                _ => {
                    readbacks.select(None);
                    feedback.set_label(if chosen.is_empty() {
                        "Select a key to see its saved settings."
                    } else {
                        "Apply uses the values shown for every selected key."
                    });
                }
            }
        }
    };
    for (_, button) in buttons.iter() {
        let update = update.clone();
        button.connect_toggled(move |_| update());
    }
    let readback = (mode.clone(), rapid.clone(), input.clone(), feedback.clone());
    let limit_cell = limit.clone();
    let enable = apply.clone();
    let ranges = input.clone();
    let observed = displayed.clone();
    let weak_buttons = Rc::downgrade(&buttons);
    readbacks.on_snapshot(move |snapshot| {
        limit_cell.set(snapshot.limit);
        if let Some(maximum) = snapshot.limit {
            for scale in &ranges {
                scale.set_range(0.1, f64::from(maximum) / 10.0);
            }
        }
        let count = weak_buttons
            .upgrade()
            .map_or(0, |buttons| selected(&buttons).len());
        enable.set_sensitive(
            snapshot.limit.is_some() && (count > 1 || (count == 1 && observed.get().is_some())),
        );
    });
    let enable = apply.clone();
    let limit_cell = limit.clone();
    readbacks.on_key(move |state| match state {
        KeyState::Confirmed(value) => {
            if displayed.replace(Some(value)) != Some(value) {
                show_readback(&readback, value);
            }
            enable.set_sensitive(limit_cell.get().is_some());
        }
        KeyState::Pending | KeyState::Unavailable => {
            displayed.set(None);
            readback.3.set_label(if state == KeyState::Pending {
                "Loading saved settings…"
            } else {
                "Saved settings could not be read."
            });
            enable.set_sensitive(false);
        }
    });

    let readbacks = Rc::downgrade(readbacks);
    let tasks = tasks.clone();
    apply.connect_clicked(move |_| {
        let chosen = selected(&buttons);
        if chosen.is_empty() || limit.get().is_none() {
            return;
        }
        let value = |i: usize| -> u8 { (input[i].value() * 10.0).round() as u8 };
        let thresholds = |offset| {
            if mode.is_active() {
                Thresholds::Separate {
                    press: value(offset),
                    release: value(offset + 1),
                }
            } else {
                Thresholds::Shared(value(offset))
            }
        };
        let tuning = Tuning {
            actuation: thresholds(0),
            rapid: rapid.is_active().then(|| thresholds(2)),
        };
        let feedback = feedback.clone();
        let readbacks = readbacks.clone();
        let edited = chosen.clone();
        tasks.keyboard(
            "Applying trigger settings…",
            move |keyboard| {
                keyboard.apply(&chosen, tuning)?;
                if let [key] = &chosen[..] {
                    Ok(Some(keyboard.read_key(*key)?))
                } else {
                    Ok(None)
                }
            },
            move |reading| {
                if let Some(state) = reading {
                    if let Some(readbacks) = readbacks.upgrade() {
                        readbacks.confirm(edited[0], state);
                    }
                    let actual = tuning_from_readback(state);
                    feedback.set_label(&if actual == tuning {
                        format!(
                            "Confirmed on keyboard · press {:.1} mm, release {:.1} mm",
                            f64::from(state.press) / 10.0,
                            f64::from(state.release) / 10.0
                        )
                    } else {
                        format!(
                            "The keyboard saved press {:.1} mm and release {:.1} mm.",
                            f64::from(state.press) / 10.0,
                            f64::from(state.release) / 10.0
                        )
                    });
                } else {
                    if let Some(readbacks) = readbacks.upgrade() {
                        readbacks.invalidate(&edited);
                    }
                    feedback.set_label("Settings sent. Select one key to verify its values.");
                }
            },
        );
    });
    page
}

fn wire_travel(
    button: &gtk::Button,
    buttons: &Rc<Vec<(Position, gtk::ToggleButton)>>,
    travel: &gtk::Label,
    tasks: &Rc<Tasks>,
    locked: [&gtk::Widget; 2],
) {
    let locked = locked.map(gtk::Widget::downgrade);
    let tracking = Rc::new(Cell::new(false));
    let buttons = buttons.clone();
    let travel = travel.clone();
    let tasks = tasks.clone();
    button.connect_clicked(move |button| {
        if tracking.get() {
            tasks.cancel.store(true, Ordering::Relaxed);
            button.set_sensitive(false);
            return;
        }
        if tasks.busy.get() {
            return;
        }
        let chosen = selected(&buttons);
        let [key] = chosen[..] else { return };
        travel.set_visible(true);
        travel.set_label("Press the selected key to see its travel reading.");
        let latest = Arc::new(AtomicU16::new(256));
        let copy = latest.clone();
        let done = tasks.clone();
        tracking.set(true);
        button.set_label("Stop travel");
        let disabled: Vec<_> = locked
            .iter()
            .filter_map(|widget| widget.upgrade())
            .collect();
        for widget in &disabled {
            widget.set_sensitive(false);
        }
        let done_tracking = tracking.clone();
        let button = button.downgrade();
        tasks.track(
            move |device, cancel| {
                let mut monitor = device.keyboard()?.monitor_travel(key)?;
                let result = (|| {
                    while !cancel.load(Ordering::Relaxed) {
                        if let Some(sample) = monitor.next_reading(Duration::from_millis(100))? {
                            latest.store(u16::from(sample), Ordering::Relaxed);
                        }
                    }
                    Ok(())
                })();
                let stop = monitor.stop();
                result.and(stop)
            },
            move |result| {
                done_tracking.set(false);
                if let Some(button) = button.upgrade() {
                    button.set_label("Track travel");
                    button.set_sensitive(true);
                }
                for widget in disabled {
                    widget.set_sensitive(true);
                }
                match result {
                    Ok(()) => done.message("Travel tracking stopped", false),
                    Err(error) => done.error(error),
                }
            },
        );
        let weak = Rc::downgrade(&tasks);
        let travel = travel.downgrade();
        glib::timeout_add_local(Duration::from_millis(40), move || {
            let (Some(tasks), Some(travel)) = (weak.upgrade(), travel.upgrade()) else {
                return glib::ControlFlow::Break;
            };
            let sample = copy.swap(256, Ordering::Relaxed);
            if sample < 256 {
                travel.set_label(&format!("Live travel reading · {sample}"));
            }
            if tasks.busy.get() {
                glib::ControlFlow::Continue
            } else {
                glib::ControlFlow::Break
            }
        });
    });
}

fn selected(buttons: &[(Position, gtk::ToggleButton)]) -> Vec<Position> {
    buttons
        .iter()
        .filter_map(|(pos, btn)| btn.is_active().then_some(*pos))
        .collect()
}

fn distance() -> gtk::Scale {
    let scale = gtk::Scale::with_range(Orientation::Horizontal, 0.1, 4.0, 0.1);
    scale.set_digits(1);
    scale.set_draw_value(true);
    scale.set_width_request(220);
    scale
}

fn tuning_from_readback(state: Readback) -> Tuning {
    let separate = state.mode & 1 != 0;
    let thresholds = |press, release| {
        if separate {
            Thresholds::Separate { press, release }
        } else {
            Thresholds::Shared(press)
        }
    };
    Tuning {
        actuation: thresholds(state.press, state.release),
        rapid: (state.mode & 0x0c != 0).then(|| thresholds(state.rapid_press, state.rapid_release)),
    }
}

fn show_readback(
    form: &(
        gtk::CheckButton,
        gtk::CheckButton,
        [gtk::Scale; 4],
        gtk::Label,
    ),
    state: Readback,
) {
    let (mode, rapid, input, status) = form;
    rapid.set_active(state.mode & 0x0c != 0);
    mode.set_active(state.mode & 1 != 0);
    for (spin, value) in input.iter().zip([
        state.press,
        state.release,
        state.rapid_press,
        state.rapid_release,
    ]) {
        spin.set_value(f64::from(value) / 10.0);
    }
    status.set_label("Loaded from the keyboard");
}

pub(super) fn device(
    model: &Connected,
    tasks: &Rc<Tasks>,
    window: &gtk::ApplicationWindow,
    readbacks: &Rc<Readbacks>,
    columns: &gtk::Adjustment,
) -> gtk::Box {
    let page = page("Device", "preferences-system-symbolic");
    let info = panel("Connection");
    let link = label("Checking connection…", "");

    info.append(&link);
    readbacks.on_snapshot(move |snapshot| {
        link.set_label(&match snapshot.link {
            Some(status) => match status.transport {
                0 => "Connected over USB".into(),
                1 => format!("Connected over 2.4 GHz · battery {}%", status.battery_level),
                value => format!("Transport {value} · battery {}", status.battery_level),
            },
            None => "Connection details unavailable".into(),
        });
    });
    page.append(&info);
    page.append(&sleep_panel(tasks, readbacks));

    if model.layout.magnetic() {
        page.append(&tuning_panel(tasks, readbacks));
        page.append(&calibration_panel(model.layout, tasks, window, columns));
    }

    let actions = panel("Device actions");
    let win =
        ["Windows key", "Alt + Tab", "Shift + Tab", "Alt + F4"].map(gtk::CheckButton::with_label);
    let shortcuts = gtk::Box::new(Orientation::Horizontal, 9);
    for option in &win {
        shortcuts.append(option);
    }
    let lock = gtk::Button::with_label("Apply shortcut locks");
    lock.set_halign(gtk::Align::End);
    actions.append(&label("Disable shortcuts while gaming", "muted"));
    actions.append(&shortcuts);
    actions.append(&lock);
    let runner = tasks.clone();
    lock.connect_clicked(move |_| {
        let [windows_key, alt_tab, shift_tab, alt_f4] =
            win.each_ref().map(|option| u8::from(option.is_active()));
        let done = runner.clone();
        runner.keyboard(
            "Setting shortcut locks…",
            move |keyboard| {
                keyboard.set_shortcut_lock(WinLock {
                    windows_key,
                    alt_tab,
                    shift_tab,
                    alt_f4,
                })
            },
            move |()| {
                done.message("Shortcut lock settings sent", false);
            },
        );
    });
    if model.gamepad() {
        let options = gtk::ComboBoxText::new();
        for (number, name) in ["Disabled", "Xbox 360", "Classic"].into_iter().enumerate() {
            options.append(Some(&number.to_string()), name);
        }
        match model.usb_id.product {
            0x7472 => options.set_active(Some(0)),
            0x7473 => options.set_active(Some(1)),
            0x7474 => options.set_active(Some(2)),
            _ => (),
        }
        let change = gtk::Button::with_label("Set gamepad mode");
        actions.append(&field("Gamepad", &options));
        actions.append(&change);
        let tasks = tasks.clone();
        let window = window.downgrade();
        change.connect_clicked(move |_| {
            let Some(mode) = options.active() else { return };
            let runner = tasks.clone();
            confirm(
                &tasks,
                &window,
                "Change gamepad mode?",
                "The keyboard will restart its USB connection.",
                "Change mode",
                move || {
                    let done = runner.clone();
                    runner.keyboard(
                        "Sending gamepad mode…",
                        move |keyboard| keyboard.set_gamepad_mode(mode as u8),
                        move |()| {
                            done.message(
                                "Gamepad mode sent. Reconnect the keyboard if needed.",
                                false,
                            );
                        },
                    );
                },
            );
        });
    }
    let reset = gtk::Button::with_label("Restore device defaults…");
    reset.set_halign(gtk::Align::Start);
    actions.append(&reset);
    wire_reset(&reset, tasks, window, readbacks, false);
    if model.layout.magnetic() {
        let reset = gtk::Button::with_label("Restore actuation defaults…");
        reset.set_halign(gtk::Align::Start);
        actions.append(&reset);
        wire_reset(&reset, tasks, window, readbacks, true);
    }
    page.append(&actions);
    page
}

fn wire_reset(
    button: &gtk::Button,
    tasks: &Rc<Tasks>,
    window: &gtk::ApplicationWindow,
    readbacks: &Rc<Readbacks>,
    magnetic: bool,
) {
    let window = window.downgrade();
    let tasks = tasks.clone();
    let readbacks = Rc::downgrade(readbacks);
    button.connect_clicked(move |_| {
        let readbacks = readbacks.clone();
        let runner = tasks.clone();
        let detail = if magnetic {
            "This replaces your current actuation settings."
        } else {
            "This replaces settings stored on the keyboard."
        };
        confirm(
            &tasks,
            &window,
            "Restore defaults?",
            detail,
            "Restore defaults",
            move || {
                let done = runner.clone();
                runner.start(
                    "Restoring defaults…",
                    false,
                    move |device, _| device.restore_defaults(magnetic),
                    move |result| {
                        match result {
                            Ok(()) => {
                                done.message("Reset sent. Refreshing device settings…", false)
                            }
                            Err(error) => done.error(error),
                        }
                        if let Some(readbacks) = readbacks.upgrade() {
                            readbacks.invalidate_all();
                        }
                    },
                );
            },
        );
    });
}

fn sleep_panel(tasks: &Rc<Tasks>, readbacks: &Readbacks) -> gtk::Box {
    let panel = panel("Idle timers");
    let lights = gtk::SpinButton::with_range(0.0, 65535.0, 1.0);
    let keyboard = gtk::SpinButton::with_range(0.0, 65535.0, 1.0);
    let write = primary("Apply timers");
    write.set_halign(gtk::Align::End);
    write.set_sensitive(false);
    panel.append(&field("Turn off lighting after · seconds", &lights));
    panel.append(&field("Put keyboard to sleep after · seconds", &keyboard));
    panel.append(&write);
    let displayed = Rc::new(Cell::new(None::<Sleep>));
    let (light_values, keyboard_values, apply, previous) = (
        lights.clone(),
        keyboard.clone(),
        write.clone(),
        displayed.clone(),
    );
    readbacks.on_snapshot(move |snapshot| {
        apply.set_sensitive(snapshot.sleep.is_some());
        if let Some(state) = snapshot.sleep
            && previous.get() != Some(state)
        {
            light_values.set_value(state.lights_seconds.into());
            keyboard_values.set_value(state.keyboard_seconds.into());
            previous.set(Some(state));
        }
    });
    let runner = tasks.clone();
    write.connect_clicked(move |_| {
        let Some(previous) = displayed.get() else {
            return;
        };
        let requested = Sleep {
            lights_seconds: lights.value_as_int() as u16,
            keyboard_seconds: keyboard.value_as_int() as u16,
            ..previous
        };
        if requested == previous {
            runner.message("Idle timers are unchanged", false);
            return;
        }
        let done = runner.clone();
        let displayed = displayed.clone();
        let lights = lights.clone();
        let keyboard = keyboard.clone();
        runner.keyboard(
            "Setting idle timers…",
            move |keyboard| {
                keyboard.set_sleep(requested)?;
                keyboard.read_sleep()
            },
            move |actual| {
                displayed.set(Some(actual));
                lights.set_value(actual.lights_seconds.into());
                keyboard.set_value(actual.keyboard_seconds.into());
                done.message(
                    if actual == requested {
                        "Idle timers confirmed"
                    } else {
                        "The keyboard returned different idle timer values."
                    },
                    actual != requested,
                );
            },
        );
    });
    panel
}

fn tuning_panel(tasks: &Rc<Tasks>, readbacks: &Readbacks) -> gtk::Box {
    let panel = panel("Keyboard performance");
    let shake = gtk::SpinButton::with_range(0.0, 255.0, 1.0);
    shake.set_numeric(true);
    let input = shake.clone();
    let output = shake.clone();
    panel.append(&control_row(
        "Anti-shake level",
        Control::AntiShake,
        &shake,
        tasks,
        readbacks,
        move || Some(input.value_as_int() as u8),
        move |value| output.set_value(value.into()),
    ));

    let rate = gtk::ComboBoxText::new();
    for value in [1, 2, 4, 8] {
        rate.append(Some(&value.to_string()), &format!("{value} kHz"));
    }
    let input = rate.clone();
    let output = rate.clone();
    panel.append(&control_row(
        "Return rate",
        Control::ReturnRate,
        &rate,
        tasks,
        readbacks,
        move || input.active_id().and_then(|value| value.parse().ok()),
        move |value| {
            if !output.set_active_id(Some(&value.to_string())) {
                output.set_active(None);
            }
        },
    ));
    panel
}

fn control_row(
    name: &'static str,
    control: Control,
    widget: &impl IsA<gtk::Widget>,
    tasks: &Rc<Tasks>,
    readbacks: &Readbacks,
    get: impl Fn() -> Option<u8> + 'static,
    show: impl Fn(u8) + Clone + 'static,
) -> gtk::Box {
    let apply = gtk::Button::with_label("Apply");
    apply.set_sensitive(false);
    let row = gtk::Box::new(Orientation::Horizontal, 7);
    row.append(widget);
    row.append(&apply);
    let observed = Rc::new(Cell::new(None));
    let previous = observed.clone();
    let enable = apply.clone();
    let display = show.clone();
    readbacks.on_snapshot(move |snapshot| {
        let value = match control {
            Control::AntiShake => snapshot.anti_shake,
            Control::ReturnRate => snapshot.return_rate,
        };
        enable.set_sensitive(value.is_some());
        if let Some(value) = value
            && previous.get() != Some(value)
        {
            previous.set(Some(value));
            display(value);
        }
    });
    let runner = tasks.clone();
    apply.connect_clicked(move |_| {
        let Some(value) = get() else { return };
        let done = runner.clone();
        let show = show.clone();
        let observed = observed.clone();
        runner.keyboard(
            "Applying performance setting…",
            move |keyboard| {
                keyboard.set_control(control, value)?;
                keyboard.read_control(control)
            },
            move |actual| {
                observed.set(Some(actual));
                show(actual);
                done.message(
                    &format!("Keyboard reported {name} · {actual}"),
                    actual != value,
                );
            },
        );
    });
    field(name, &row)
}

fn calibration_panel(
    layout: &'static Layout,
    tasks: &Rc<Tasks>,
    window: &gtk::ApplicationWindow,
    columns: &gtk::Adjustment,
) -> gtk::Box {
    let panel = panel("Calibration");
    let picker = gtk::MenuButton::builder().label("Choose a key").build();
    let popover = gtk::Popover::new();
    picker.set_popover(Some(&popover));
    picker.update_property(&[gtk::accessible::Property::Label("Choose a calibration key")]);
    let all = gtk::CheckButton::with_label("All keys");
    all.set_active(true);
    let flow = gtk::FlowBox::builder()
        .homogeneous(true)
        .selection_mode(gtk::SelectionMode::None)
        .column_spacing(8)
        .row_spacing(4)
        .build();
    super::responsive::flow_columns(&flow, columns);
    flow.insert(&all, -1);
    let selected = Rc::new(Cell::new(None));
    let selection = selected.clone();
    let menu = picker.downgrade();
    all.connect_toggled(move |all| {
        if all.is_active() {
            selection.set(None);
            if let Some(menu) = menu.upgrade() {
                menu.set_label("All keys");
                menu.popdown();
            }
        }
    });
    let map = dragon_driver::witmod_sdk::keymap::KeyMap::default();
    for (pos, code) in layout.keys() {
        let caption = super::keys::caption(&map, code);
        let key = gtk::CheckButton::with_label(&caption);
        key.set_group(Some(&all));
        let name = super::keys::accessible_name(&map, code);
        key.set_tooltip_text(Some(&name));
        key.update_property(&[gtk::accessible::Property::Label(&name)]);
        let selected = selected.clone();
        let menu = picker.downgrade();
        key.connect_toggled(move |key| {
            if key.is_active() {
                selected.set(Some(pos));
                if let Some(menu) = menu.upgrade() {
                    menu.set_label(&caption);
                    menu.popdown();
                }
            }
        });
        flow.insert(&key, -1);
    }
    let scroll = gtk::ScrolledWindow::builder()
        .hscrollbar_policy(gtk::PolicyType::Never)
        .max_content_height(216)
        .propagate_natural_height(true)
        .child(&flow)
        .build();
    popover.set_child(Some(&scroll));
    panel.append(&field("Keys to calibrate", &picker));
    let start = gtk::Button::with_label("Start calibration…");
    start.set_halign(gtk::Align::End);
    panel.append(&start);
    let window = window.downgrade();
    let tasks = tasks.clone();
    start.connect_clicked(move |_| {
        let key = selected.get();
        let runner = tasks.clone();
        confirm(&tasks, &window, "Calibrate key travel?", "After starting, fully press and release each selected key. Calibration changes are stored on the keyboard.", "Start calibration", move || {
            runner.message("Press and release each selected key", false);
            let done = runner.clone();
            runner.start("Calibrating… Press and release each selected key", true, move |device, cancel| {
                let mut calibration = device.keyboard()?.calibrate(key)?;
                let result = (|| {
                    let start = Instant::now();
                    while !cancel.load(Ordering::Relaxed) && start.elapsed() < Duration::from_secs(120) {
                        if let Some(event) = calibration.progress(Duration::from_millis(150))? {
                            let code = match event {
                                CalibrationStatus::Single(code) | CalibrationStatus::All { status: code, .. } => code,
                            };
                            match code {
                                0 => return Ok(true),
                                1 => (),
                                _ => return Err(io::Error::other("The keyboard reported a calibration failure")),
                            }
                        }
                    }
                    if cancel.load(Ordering::Relaxed) { Ok(false) }
                    else { Err(io::Error::new(io::ErrorKind::TimedOut, "Calibration timed out")) }
                })();
                let stop = calibration.stop();
                result.and_then(|complete| stop.map(|()| complete))
            }, move |result| match result {
                Ok(true) => done.message("Calibration complete", false),
                Ok(false) => done.message("Calibration stopped", false),
                Err(error) => done.error(error),
            });
        });
    });
    panel
}
