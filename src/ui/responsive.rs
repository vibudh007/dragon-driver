use gtk::{glib, prelude::*};

fn columns(width: i32, monitor: i32, fullscreen: bool) -> u32 {
    if fullscreen {
        return 3;
    }
    if monitor <= 0 {
        return 1;
    }
    if i64::from(width) * 3 >= i64::from(monitor) * 2 {
        3
    } else if i64::from(width) * 3 >= i64::from(monitor) {
        2
    } else {
        1
    }
}

pub(super) fn monitor_columns(window: &gtk::ApplicationWindow) -> gtk::Adjustment {
    let value = gtk::Adjustment::new(1.0, 1.0, 3.0, 1.0, 1.0, 0.0);
    let weak = window.downgrade();
    let output = value.downgrade();
    let update = move |surface: &gtk::gdk::Surface, width| {
        if let (Some(window), Some(output)) = (weak.upgrade(), output.upgrade()) {
            let monitor = surface
                .display()
                .monitor_at_surface(surface)
                .map_or(0, |monitor| monitor.geometry().width());
            output.set_value(columns(width, monitor, window.is_fullscreen()).into());
        }
    };
    window.connect_realize(move |window| {
        let Some(surface) = window.surface() else {
            return;
        };
        update(&surface, surface.width());
        let resize = update.clone();
        surface.connect_layout(move |surface, width, _| resize(surface, width));
        let refresh = update.clone();
        surface.connect_enter_monitor(move |surface, _| {
            let surface = surface.downgrade();
            let refresh = refresh.clone();
            glib::idle_add_local_once(move || {
                if let Some(surface) = surface.upgrade() {
                    refresh(&surface, surface.width());
                }
            });
        });
    });
    let output = value.downgrade();
    window.connect_fullscreened_notify(move |window| {
        if let (Some(surface), Some(output)) = (window.surface(), output.upgrade()) {
            let monitor = surface
                .display()
                .monitor_at_surface(&surface)
                .map_or(0, |monitor| monitor.geometry().width());
            output.set_value(columns(surface.width(), monitor, window.is_fullscreen()).into());
        }
    });
    value
}

pub(super) fn flow_columns(flow: &gtk::FlowBox, count: &gtk::Adjustment) {
    let weak = flow.downgrade();
    let update = move |count: &gtk::Adjustment| {
        if let Some(flow) = weak.upgrade() {
            let count = count.value() as u32;
            flow.set_min_children_per_line(1);
            flow.set_max_children_per_line(count);
            flow.set_min_children_per_line(count);
        }
    };
    update(count);
    let signal = count.connect_value_changed(update);
    let count = count.downgrade();
    let signal = std::cell::Cell::new(Some(signal));
    flow.connect_destroy(move |_| {
        if let (Some(count), Some(signal)) = (count.upgrade(), signal.take()) {
            count.disconnect(signal);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::columns;

    #[test]
    fn columns_follow_monitor_width_and_fullscreen() {
        for (width, expected) in [(399, 1), (400, 2), (799, 2), (800, 3)] {
            assert_eq!(columns(width, 1200, false), expected);
        }
        assert_eq!(columns(799, 2400, false), 1);
        assert_eq!(columns(399, 1200, true), 3);
        assert_eq!(columns(399, 0, false), 1);
    }
}
