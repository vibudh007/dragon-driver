use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use dragon_driver::keyboard::models::{find as find_layout, layouts};
use dragon_driver::witmod_sdk::catalog::UsbId;
use gtk::{gio, glib, graphene, gsk, prelude::*};

use super::{Connected, build_window, install_style};

#[test]
#[ignore = "requires a graphical display; use the project Xvfb container"]
fn window_gates_controls_and_remains_responsive() {
    gtk::init().expect("GTK must initialize");
    let app = gtk::Application::new(
        Some("org.dragondriver.GtkSmoke"),
        gio::ApplicationFlags::NON_UNIQUE,
    );
    install_style(&app);
    app.register(gio::Cancellable::NONE).unwrap();
    let view = build_window(&app).unwrap();
    let window = view.window.upgrade().unwrap();
    assert_eq!(window.icon_name().as_deref(), Some(crate::APP_ID));
    reactive_switch_keeps_animating_during_unchanged_poll(&window);
    let titlebar = window.titlebar().unwrap();
    assert_eq!(window.title().as_deref(), Some(crate::APP_NAME));
    assert!(find_label(&titlebar, crate::APP_NAME).is_some());
    let logo = find(&titlebar, |image: &gtk::Image| {
        image.has_css_class("brand-icon")
    })
    .unwrap();
    assert_eq!(logo.pixel_size(), 24);
    let source = logo.gicon().unwrap().downcast::<gio::BytesIcon>().unwrap();
    assert_eq!(
        source.bytes().as_ref(),
        include_bytes!("../../packaging/org.dragondriver.DragonDriver.png")
    );
    let layout = find_layout("7153_AUSHERGB").unwrap();
    assert_eq!(view.screens.visible_child_name().as_deref(), Some("empty"));
    pump(Duration::from_millis(290));
    if let Ok(theme) = std::env::var("GTK_THEME")
        && theme.starts_with("Adwaita")
    {
        assert_eq!(
            window.has_css_class("light-control-borders"),
            theme.ends_with(":dark")
        );
    }
    screenshot(&window, "empty");
    let heading = find_label(&window, "Connect your keyboard").unwrap();
    assert_eq!(heading.xalign(), 0.5);
    let center = |widget: &gtk::Label| {
        let bounds = widget.compute_bounds(&window).unwrap();
        bounds.x() + bounds.width() / 2.0
    };
    if std::env::var_os("DRAGON_DRIVER_TEST_ALTERNATE_THEME").is_some() {
        let color = heading.style_context().color();
        assert_eq!(
            [color.red(), color.green(), color.blue()].map(|value| (value * 255.0).round() as u8),
            [19, 96, 144],
        );
    }
    let centered = center(&heading);
    view.empty_detail.set_label("Looking for your keyboard…");
    pump(Duration::from_millis(40));
    assert!((center(&heading) - centered).abs() < 2.0);
    assert!(
        find(&window, |button: &gtk::Button| button
            .icon_name()
            .as_deref()
            == Some("view-refresh-symbolic"))
        .is_none()
    );
    view.show_keyboard(Connected {
        layout,
        name: "K617 Fizz Rapid Trigger".into(),
        firmware: "V1_11_27".into(),
        node: "/dev/hidraw4294967295".into(),
        transport: Some(0),
        palettes: [None; 2],
        palette_slot: 0,
        palette_error: None,
        usb_id: UsbId {
            vendor: 0x0416,
            product: 0x7372,
        },
    });
    assert!(view.pages.child_by_name("actuation").is_some());
    assert!(view.pages.child_by_name("profiles").is_some());
    assert!(find_label(&window, "Side lighting").is_none());
    assert!(find_label(&window, "Live 2.4 GHz lighting").is_none());
    let key = find_button(&view.keybed, "W")
        .unwrap()
        .downcast::<gtk::ToggleButton>()
        .unwrap();
    if std::env::var_os("DRAGON_DRIVER_TEST_DESKTOP_ENTRY").is_some() {
        verify_drag(&view, &window);
    }
    key.set_active(true);
    assert!(find_button(&window, "Read selected key").is_none());
    assert!(find_button(&window, "Read from keyboard").is_none());
    assert!(find_button(&window, "Read connection status").is_none());
    let press = live_row::<gtk::Scale>(&window, "Press · mm");
    assert_eq!(
        (press.adjustment().lower(), press.adjustment().upper()),
        (0.1, 4.0)
    );
    assert!(find_label(&window, "1 key selected").is_some());
    assert!(
        !find_button(&window, "Apply to selected keys")
            .unwrap()
            .is_sensitive()
    );
    pump(Duration::from_millis(290));
    screenshot(&window, "actuation");
    let track = find_button(&window, "Track travel").unwrap();
    track.emit_clicked();
    assert_eq!(track.label().as_deref(), Some("Stop travel"));
    assert!(track.is_sensitive());
    assert!(!key.is_sensitive());
    track.emit_clicked();
    assert!(view.tasks.cancel.load(std::sync::atomic::Ordering::Relaxed));
    wait_live("inline tracking cleanup", || {
        track.label().as_deref() == Some("Track travel")
    });
    assert!(key.is_sensitive());
    verify_centered_keybed(&view);
    let bounds = key.compute_bounds(&window).unwrap();
    view.pages.set_visible_child_name("lighting");
    pump(Duration::from_millis(220));
    assert_eq!(key.compute_bounds(&window).unwrap(), bounds);
    assert!(key.is_active());
    verify_color_choice(&view);
    pump(Duration::from_millis(180));
    verify_centered_keybed(&view);
    let bounds = key.compute_bounds(&window).unwrap();
    view.pages.set_visible_child_name("profiles");
    pump(Duration::from_millis(290));
    screenshot(&window, "profiles");
    verify_centered_keybed(&view);
    assert_eq!(key.compute_bounds(&window).unwrap(), bounds);
    assert!(key.is_active());
    view.pages.set_visible_child_name("device");
    pump(Duration::from_millis(220));
    assert!(!view.keybed.is_visible());
    assert!(!key.is_mapped());
    let picker = find(&window, |widget: &gtk::MenuButton| {
        widget.label().as_deref() == Some("Choose a key")
    })
    .unwrap();
    picker.popup();
    pump(Duration::from_millis(100));
    let popover = picker.popover().unwrap();
    assert!(popover.is_mapped());
    let list = find::<gtk::FlowBox>(&popover, |_| true).unwrap();
    assert!(
        list.parent()
            .is_some_and(|parent| parent.is::<gtk::Viewport>())
    );
    let toggle = find(&list, |key: &gtk::CheckButton| {
        key.label().as_deref() == Some("W")
    })
    .unwrap();
    for count in [1, 2, 3] {
        view.columns.set_value(f64::from(count));
        assert_eq!(list.max_children_per_line(), count);
    }
    toggle.set_active(true);
    assert_eq!(picker.label().as_deref(), Some("W"));
    pump(Duration::from_millis(150));
    assert!(!popover.is_mapped());
    picker.popup();
    assert!(toggle.is_active());
    picker.popdown();
    view.pages.set_visible_child_name("profiles");
    pump(Duration::from_millis(200));
    assert_eq!(key.compute_bounds(&window).unwrap(), bounds);
    let key = find_button(&view.keybed, "W")
        .unwrap()
        .downcast::<gtk::ToggleButton>()
        .unwrap();
    key.set_active(true);
    let another = find_button(&view.keybed, "A")
        .unwrap()
        .downcast::<gtk::ToggleButton>()
        .unwrap();
    another.set_active(true);
    assert!(
        !find_button(&window, "Edit assignment…")
            .unwrap()
            .is_sensitive()
    );
    another.set_active(false);
    assert!(
        find_button(&window, "Edit assignment…")
            .unwrap()
            .is_sensitive()
    );
    find_button(&view.pages.visible_child().unwrap(), "Edit assignment…")
        .unwrap()
        .emit_clicked();
    pump(Duration::from_millis(140));
    let dialog = gtk::Window::list_toplevels()
        .into_iter()
        .find_map(|w| w.downcast::<gtk::Dialog>().ok())
        .unwrap();
    let action = find(&dialog, |combo: &gtk::ComboBoxText| {
        combo.active_id().as_deref() == Some("default")
    })
    .unwrap();
    assert!(action.set_active_id(Some("0")));
    dialog.response(gtk::ResponseType::Accept);
    assert!(find_label(&window, "W · Key or disabled · 4").is_some());
    find_button(&view.pages.visible_child().unwrap(), "Check layout")
        .unwrap()
        .emit_clicked();
    pump(Duration::from_millis(190));
    assert!(find_label(&window, "These key assignments are ready to save.").is_some());

    find_button(&view.pages.visible_child().unwrap(), "Edit assignment…")
        .unwrap()
        .emit_clicked();
    pump(Duration::from_millis(140));
    let dialog = gtk::Window::list_toplevels()
        .into_iter()
        .find_map(|window| window.downcast::<gtk::Dialog>().ok())
        .unwrap();
    let action = find(&dialog, |combo: &gtk::ComboBoxText| {
        combo.is_mapped() && combo.active_id().as_deref() == Some("0")
    })
    .unwrap();
    assert!(action.set_active_id(Some("6")));
    pump(Duration::from_millis(220));
    let input = find(&dialog, |input: &gtk::TextView| input.is_mapped()).unwrap();
    input.buffer().set_text("α beta");
    dialog.response(gtk::ResponseType::Accept);
    assert!(find_label(&window, "W · Paste text · α beta").is_some());
    view.tasks.desktop_action(
        dragon_driver::keyboard::desktop::DesktopBinding::Text("Copy fixture".into()),
        None,
    );
    let copy = find_button(&window, "Copy text").unwrap();
    assert!(copy.is_visible());
    copy.emit_clicked();
    assert!(!copy.is_visible());

    verify_reconnected_palette(&view, layout);

    let layout = layouts()
        .iter()
        .find(|layout| !layout.magnetic() && layout.side_lighting)
        .unwrap();
    view.show_keyboard(Connected {
        layout,
        name: "Vendor test device".into(),
        firmware: "test".into(),
        node: "/dev/hidraw4294967295".into(),
        transport: Some(1),
        palettes: [None; 2],
        palette_slot: 0,
        palette_error: None,
        usb_id: UsbId {
            vendor: 0x0416,
            product: 0x7473,
        },
    });
    assert!(view.pages.child_by_name("actuation").is_none());
    assert!(find_label(&window, "Lighting effects").is_some());
    assert!(find_label(&window, "Side lighting").is_some());
    assert!(find_label(&window, "Live 2.4 GHz lighting").is_some());
    assert!(find_label(&window, "Wallpaper-reactive lighting").is_none());
    let audio = find_label(&window, "Audio-reactive lighting")
        .unwrap()
        .parent()
        .unwrap();
    assert!(find_label(&audio, "Color palette").is_none());
    assert!(!live_row::<gtk::Switch>(&audio, "React to desktop audio").is_active());
    assert_eq!(
        view.pages.child_by_name("profiles").is_some(),
        layout.onboard_profiles
    );
    assert!(find_button(&window, "Set gamepad mode").is_some());

    let spinner = find::<gtk::Spinner>(&window, |_| true).unwrap();
    verify_background_does_not_freeze_controls(&view, &window, &spinner);
    let result = Rc::new(Cell::new(false));
    let copy = result.clone();
    view.tasks.start(
        "Slow test",
        false,
        |_, _| {
            std::thread::sleep(Duration::from_millis(850));
            Ok(())
        },
        move |value| {
            copy.set(value.is_ok());
        },
    );
    pump(Duration::from_millis(210));
    assert!(!spinner.is_spinning());
    pump(Duration::from_millis(390));
    assert!(spinner.is_spinning());
    pump(Duration::from_millis(400));
    assert!(result.get());
    assert!(!spinner.is_spinning());
    let copy = result.clone();
    result.set(false);
    view.tasks.start(
        "Quick test",
        false,
        |_, _| Ok(()),
        move |value| copy.set(value.is_ok()),
    );
    pump(Duration::from_millis(90));
    assert!(result.get());
    assert!(!spinner.is_spinning());
    if super::desktop::native_x11() {
        test_x11_clipboard();
    }
    if std::env::var_os("DRAGON_DRIVER_TEST_DESKTOP_ENTRY").is_some() {
        assert!(
            super::desktop::native_x11(),
            "desktop fixture uses isolated X11"
        );
        super::desktop::launch("org.dragondriver.OfflineFixture.desktop").unwrap();
    }
    window.close();
}

fn verify_centered_keybed(view: &super::View) {
    let figure = find(&view.keybed, |widget: &gtk::Fixed| {
        widget.has_css_class("keybed")
    })
    .unwrap();
    let viewport = find::<gtk::ScrolledWindow>(&view.keybed, |_| true).unwrap();
    let selecting = find(&view.keybed, |button: &gtk::Button| {
        button.tooltip_text().as_deref() == Some("Select all")
    })
    .unwrap();
    let figure = figure.compute_bounds(&view.keybed).unwrap();
    let viewport = viewport.compute_bounds(&view.keybed).unwrap();
    let selecting = selecting.compute_bounds(&view.keybed).unwrap();
    let center = |bounds: graphene::Rect| bounds.x() + bounds.width() / 2.0;
    let width = view.keybed.width() as f32;
    assert!((center(viewport) - width / 2.0).abs() <= 1.0);
    assert!((center(figure) - width / 2.0).abs() <= 1.0);
    assert!(selecting.x() >= figure.x() + figure.width());
}

fn verify_color_choice(view: &super::View) {
    use dragon_driver::keyboard::lighting::{Effect, SupportedModes};

    let readbacks = view.readbacks.borrow().as_ref().unwrap().clone();
    readbacks.publish(super::readbacks::Snapshot {
        lighting: Some((
            SupportedModes {
                modes: vec![0, 10],
                settings: [4, 4],
            },
            Effect {
                mode: 10,
                brightness: 4,
                speed: 1,
                foreground: [0; 3],
                background: [0; 3],
                direction: 0,
                effect_type: 0,
                open: 1,
            },
        )),
        ..Default::default()
    });
    let chooser = swatch(&view.keybed, "Choose key color");
    assert!(
        chooser.is_sensitive(),
        "one selected key enables the picker"
    );
    let panel = view.pages.visible_child().unwrap();
    let wallpaper = live_row::<gtk::Switch>(&panel, "Match your wallpaper");
    assert!(wallpaper.is_sensitive());
    assert!(!wallpaper.is_active());
    let audio = find_label(&panel, "Audio-reactive lighting")
        .unwrap()
        .parent()
        .unwrap();
    let audio_switch = live_row::<gtk::Switch>(&audio, "React to desktop audio");
    assert!(audio_switch.is_sensitive());
    for toggle in [&wallpaper, &audio_switch] {
        toggle.set_state(true);
        assert!(toggle.state());
        super::controls::sync_switch(toggle, false);
        assert!(!toggle.is_active());
        assert!(!toggle.state());
    }
    let palette_source = live_row::<gtk::ComboBoxText>(&audio, "Color palette");
    assert_eq!(palette_source.active_id().as_deref(), Some("manual"));
    for name in ["Background", "Low", "Medium", "High"] {
        assert!(find_label(&audio, name).is_some());
    }
    palette_source.set_active_id(Some("wallpaper"));
    assert!(
        !find_label(&audio, "Background")
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .is_visible()
    );
    palette_source.set_active_id(Some("manual"));
    let [red, green, blue] = key_fill_rgb(&find_button(&view.keybed, "W").unwrap());
    assert!(
        red > green && red > blue,
        "the default key fill should be red"
    );
    let brightness = find(&view.keybed, |slider: &gtk::Scale| {
        slider.tooltip_text().as_deref() == Some("Keyboard brightness")
    })
    .unwrap();
    assert!(brightness.is_sensitive());
    assert!(find_label(&view.keybed, "Choose color").is_none());
    assert!(find_label(&view.keybed, "Keyboard brightness").is_none());
    chooser.emit_clicked();
    let picker = wheel_dialog(&chooser);
    let hex = find::<gtk::Entry>(&picker, |_| true).unwrap();
    assert_eq!(hex.text(), "#FF0000");
    hex.set_text("#32AB47");
    assert!(
        find_button(&panel, "Apply color to all keys")
            .unwrap()
            .is_visible()
    );
    find_button(&picker, "Done").unwrap().emit_clicked();
    find(&view.keybed, |button: &gtk::Button| {
        button.tooltip_text().as_deref() == Some("Clear selection")
    })
    .unwrap()
    .emit_clicked();
    pump(Duration::from_millis(50));
    assert!(
        chooser.is_sensitive(),
        "the brush color can be chosen with no keys selected"
    );
    chooser.emit_clicked();
    let picker = wheel_dialog(&chooser);
    find::<gtk::Entry>(&picker, |_| true)
        .unwrap()
        .set_text("#45D87F");
    assert!(find_label(&panel, "Changing key colors…").is_none());
    find_button(&picker, "Done").unwrap().emit_clicked();
    assert!(
        brightness.is_sensitive(),
        "brightness is independent of selection"
    );
    find_button(&view.keybed, "W")
        .unwrap()
        .downcast::<gtk::ToggleButton>()
        .unwrap()
        .set_active(true);

    let w = find_button(&view.keybed, "W")
        .unwrap()
        .downcast::<gtk::ToggleButton>()
        .unwrap();
    let a = find_button(&view.keybed, "A")
        .unwrap()
        .downcast::<gtk::ToggleButton>()
        .unwrap();
    let fill = find_button(&panel, "Apply color to all keys").unwrap();
    assert!(w.is_active());
    assert!(!a.is_active());
    fill.emit_clicked();
    assert!(
        w.is_active(),
        "initializing the palette must preserve selection"
    );
    assert!(
        !a.is_active(),
        "initializing the palette must not select the rest of the keys"
    );
    wait_live("offline full-palette attempt", || {
        find_label(
            &panel,
            "Couldn’t confirm the new colors. Reconnect or apply a new palette.",
        )
        .is_some()
    });

    assert!(find_label(&panel, "Lighting effects").is_some());
    assert!(find_label(&panel, "Effect").unwrap().is_mapped());
    let show_effect = |mode| {
        readbacks.publish(super::readbacks::Snapshot {
            lighting: Some((
                SupportedModes {
                    modes: vec![0, 2, 3, 4, 10],
                    settings: [4, 4],
                },
                Effect {
                    mode,
                    brightness: 4,
                    speed: 1,
                    foreground: [0; 3],
                    background: [0; 3],
                    direction: 0,
                    effect_type: u8::from(mode != 0),
                    open: 1,
                },
            )),
            ..Default::default()
        })
    };
    show_effect(0);
    let rainbow = find(&view.keybed, |button: &gtk::ToggleButton| {
        button.has_css_class("rainbow-swatch")
    })
    .unwrap();
    let color = swatch(&view.keybed, "Choose effect color");
    let background = swatch(&view.keybed, "Choose background color");
    assert!(!rainbow.is_visible());
    assert!(!background.is_visible());
    assert_eq!(rainbow.parent(), color.parent());
    let direction = find_label(&panel, "Direction").unwrap().parent().unwrap();
    assert!(!direction.is_visible());
    assert!(find_label(&panel, "Lighting on").is_none());
    assert!(
        find(&panel, |widget: &gtk::Expander| {
            widget.label().as_deref() == Some("Effect options")
        })
        .is_none()
    );
    assert!(find_label(&panel, "Paint color").unwrap().is_visible());
    assert!(find_label(&panel, "Background color").is_none());
    assert!(find_label(&panel, "Color").is_none());
    assert!(w.is_active());
    chooser.emit_clicked();
    let picker = wheel_dialog(&chooser);
    find::<gtk::Entry>(&picker, |_| true)
        .unwrap()
        .set_text("#123456");
    assert!(find_label(&panel, "Swatch ready. Select more keys to paint.").is_some());
    assert!(find_label(&panel, "Changing key colors…").is_none());
    find_button(&picker, "Done").unwrap().emit_clicked();

    find_button(&view.keybed, "A")
        .unwrap()
        .downcast::<gtk::ToggleButton>()
        .unwrap()
        .set_active(true);
    while glib::MainContext::default().pending() {
        glib::MainContext::default().iteration(false);
    }
    assert!(find_label(&panel, "Changing key colors…").is_some());
    wait_live("offline brush attempt", || {
        find_label(
            &panel,
            "Couldn’t confirm the new colors. Reconnect or apply a new palette.",
        )
        .is_some()
    });
    find_button(&view.keybed, "A")
        .unwrap()
        .downcast::<gtk::ToggleButton>()
        .unwrap()
        .set_active(false);
    show_effect(3);
    assert!(rainbow.is_visible());
    assert!(!background.is_visible());
    assert!(!direction.is_visible());
    let direction_button = |caption| {
        find(&direction, |button: &gtk::ToggleButton| {
            button.tooltip_text().as_deref() == Some(caption)
        })
        .unwrap()
    };
    show_effect(4);
    assert!(direction.is_visible());
    assert!(direction_button("Clockwise").is_active());
    assert!(direction_button("Counterclockwise").is_visible());
    assert!(!direction_button("Left to right").is_visible());
    show_effect(2);
    assert!(rainbow.is_active());
    assert!(background.is_visible());
    assert!(direction_button("Left to right").is_active());
    assert!(direction_button("Inward").is_visible());
    assert!(!direction_button("Clockwise").is_visible());
    assert_eq!(
        rainbow.next_sibling().as_ref(),
        Some(background.upcast_ref())
    );
    assert_eq!(background.next_sibling().as_ref(), Some(color.upcast_ref()));
    let size = |button: &gtk::Button| {
        let area = button
            .child()
            .unwrap()
            .downcast::<gtk::DrawingArea>()
            .unwrap();
        (area.content_width(), area.content_height())
    };
    assert_eq!(size(rainbow.upcast_ref()), size(&color));
    let before = brightness.value();
    let picker = choose_wheel_color(&color);
    assert_eq!(brightness.value(), before);
    find_button(&picker, "Done").unwrap().emit_clicked();
    assert!(
        !rainbow.is_active(),
        "choosing a color must also switch the effect out of rainbow mode"
    );
}

fn verify_reconnected_palette(
    view: &super::View,
    layout: &'static dragon_driver::keyboard::models::Layout,
) {
    use dragon_driver::keyboard::lighting::{Effect, RgbFrame, SupportedModes};

    let mut palette = RgbFrame::default();
    palette[2][2] = [0, 255, 0];
    view.show_keyboard(Connected {
        layout,
        name: "Reconnected K617".into(),
        firmware: "V1_11_27".into(),
        node: "/dev/hidraw4294967294".into(),
        transport: Some(0),
        palettes: [None, Some(palette)],
        palette_slot: 1,
        palette_error: None,
        usb_id: UsbId {
            vendor: 0x0416,
            product: 0x7372,
        },
    });
    view.pages.set_visible_child_name("lighting");
    pump(Duration::from_millis(220));
    let readbacks = view.readbacks.borrow().as_ref().unwrap().clone();
    readbacks.publish(super::readbacks::Snapshot {
        lighting: Some((
            SupportedModes {
                modes: vec![0, 10],
                settings: [4, 4],
            },
            Effect {
                mode: 10,
                brightness: 5,
                speed: 1,
                foreground: [0; 3],
                background: [0; 3],
                direction: 0,
                effect_type: 0,
                open: 0,
            },
        )),
        ..Default::default()
    });
    let panel = view.pages.visible_child().unwrap();
    assert!(find_label(&panel, "Select keys to paint with the current swatch.").is_some());
    let w = find_button(&view.keybed, "W").unwrap();
    let [red, green, blue] = key_fill_rgb(&w);
    assert!(
        green > red && green > blue,
        "the saved key fill should be green"
    );
    w.downcast::<gtk::ToggleButton>().unwrap().set_active(true);
    pump(Duration::from_millis(40));
    assert!(find_label(&panel, "Changing key colors…").is_some());
    wait_live("reconnected palette painting", || {
        find_label(
            &panel,
            "Couldn’t confirm the new colors. Reconnect or apply a new palette.",
        )
        .is_some()
    });
}

fn reactive_switch_keeps_animating_during_unchanged_poll(parent: &gtk::ApplicationWindow) {
    let settings = gtk::Settings::default().unwrap();
    let animations = settings.is_gtk_enable_animations();
    settings.set_gtk_enable_animations(true);
    let toggle = gtk::Switch::new();
    let window = gtk::Window::builder()
        .transient_for(parent)
        .child(&toggle)
        .build();
    window.present();
    pump(Duration::from_millis(100));

    for next in [true, false] {
        assert_ne!(toggle.is_active(), next);
        toggle.emit_activate();
        // The daemon's next poll still describes the state before the click.
        super::controls::sync_switch(&toggle, !next);
        pump(Duration::from_millis(260));
        assert_eq!(
            toggle.is_active(),
            next,
            "unchanged polling cancelled the switch animation"
        );
        assert_eq!(toggle.state(), next);
    }
    window.destroy();
    settings.set_gtk_enable_animations(animations);
}

fn key_fill_rgb(button: &gtk::Button) -> [u8; 3] {
    let width = button.width() as usize;
    let height = button.height() as usize;
    let paintable = gtk::WidgetPaintable::new(Some(button));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, width as f64, height as f64);
    let node = snapshot.to_node().expect("the key should be rendered");
    let window = button.root().unwrap().downcast::<gtk::Window>().unwrap();
    let renderer = gsk::Renderer::for_surface(&window.surface().unwrap()).unwrap();
    let bounds = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
    let texture = renderer.render_texture(&node, Some(&bounds));
    let mut bytes = vec![0; width * height * 4];
    texture.download(&mut bytes, width * 4);
    renderer.unrealize();
    let offset = (height / 3 * width + width / 5) * 4;
    let pixel = u32::from_ne_bytes(bytes[offset..offset + 4].try_into().unwrap());
    [(pixel >> 16) as u8, (pixel >> 8) as u8, pixel as u8]
}

fn swatch(root: &impl IsA<gtk::Widget>, tooltip: &str) -> gtk::Button {
    find(root, |button: &gtk::Button| {
        button.has_css_class("color-wheel-button")
            && button.tooltip_text().as_deref() == Some(tooltip)
    })
    .unwrap()
}

fn wheel_dialog(button: &gtk::Button) -> gtk::Window {
    pump(Duration::from_millis(120));
    let title = button.tooltip_text();
    gtk::Window::list_toplevels()
        .into_iter()
        .filter_map(|widget| widget.downcast::<gtk::Window>().ok())
        .find(|window| window.is_mapped() && window.title() == title)
        .unwrap()
}

fn choose_wheel_color(button: &gtk::Button) -> gtk::Window {
    pump(Duration::from_millis(90));
    let swatch = button
        .child()
        .unwrap()
        .downcast::<gtk::DrawingArea>()
        .unwrap();
    assert!(swatch.is_mapped());
    assert!(swatch.width() >= 38 && swatch.height() >= 28);
    let parent = button.parent().unwrap();
    let bounds = button.compute_bounds(&parent).unwrap();
    assert!((bounds.width() - bounds.height() * 4.0 / 3.0).abs() <= 1.0);
    if std::env::var_os("DRAGON_DRIVER_TEST_DESKTOP_ENTRY").is_some() {
        assert!(
            std::process::Command::new("/usr/bin/xdotool")
                .args(["mousemove", "1", "1"])
                .status()
                .unwrap()
                .success()
        );
    }
    button.emit_clicked();
    let dialog = wheel_dialog(button);
    let wheel = find::<gtk::DrawingArea>(&dialog, |_| true).unwrap();
    assert!(wheel.is_mapped());
    assert!(find_button(&dialog, "Use color").is_none());
    let entry = find::<gtk::Entry>(&dialog, |_| true).unwrap();
    let before = entry.text();
    assert_eq!(before, "#000000");
    screenshot(&dialog, "color-wheel");
    if std::env::var_os("DRAGON_DRIVER_TEST_DESKTOP_ENTRY").is_some() {
        let area = wheel.compute_bounds(&dialog).unwrap();
        let xid = super::desktop::x11_test_focus(&dialog).to_string();
        let x = (area.x() as i32 + 20).to_string();
        let y = (area.y() as i32 + 70).to_string();
        let mut pointer = std::process::Command::new("/usr/bin/xdotool")
            .args(["mousemove", "--window", &xid, &x, &y])
            .spawn()
            .unwrap();
        pump(Duration::from_millis(190));
        assert!(pointer.wait().unwrap().success());
    } else {
        let controllers = wheel.observe_controllers();
        let motion = (0..controllers.n_items())
            .find_map(|index| {
                controllers
                    .item(index)?
                    .downcast::<gtk::EventControllerMotion>()
                    .ok()
            })
            .unwrap();
        motion.emit_by_name::<()>("motion", &[&20.0f64, &70.0f64]);
    }
    assert_ne!(entry.text(), before);
    let controllers = wheel.observe_controllers();
    let motion = (0..controllers.n_items())
        .find_map(|index| {
            controllers
                .item(index)?
                .downcast::<gtk::EventControllerMotion>()
                .ok()
        })
        .unwrap();
    let click = (0..controllers.n_items())
        .find_map(|index| {
            controllers
                .item(index)?
                .downcast::<gtk::GestureClick>()
                .ok()
        })
        .unwrap();
    click.emit_by_name::<()>("released", &[&1i32, &20.0f64, &70.0f64]);
    let pinned = entry.text();
    motion.emit_by_name::<()>("motion", &[&175.0f64, &60.0f64]);
    assert_eq!(entry.text(), pinned);
    click.emit_by_name::<()>("released", &[&1i32, &175.0f64, &60.0f64]);
    let resumed = entry.text();
    motion.emit_by_name::<()>("motion", &[&175.0f64, &135.0f64]);
    assert_ne!(entry.text(), resumed);
    entry.set_text("invalid");
    assert!(entry.has_css_class("error"));
    entry.set_text("#FF0080");
    assert!(!entry.has_css_class("error"));
    let slider = live_row::<gtk::Scale>(&dialog, "Color brightness");
    slider.set_value(50.0);
    assert_eq!(entry.text(), "#800040");
    dialog
}

fn verify_drag(view: &super::View, window: &gtk::ApplicationWindow) {
    pump(Duration::from_millis(230));
    let key = |caption| {
        find_button(&view.keybed, caption)
            .unwrap()
            .downcast::<gtk::ToggleButton>()
            .unwrap()
    };
    let [q, w, e] = [key("Q"), key("W"), key("E")];
    let location = |key: &gtk::ToggleButton| {
        let xy = key.compute_bounds(window).unwrap();
        [
            ((xy.x() + xy.width() / 2.0) as i32).to_string(),
            ((xy.y() + xy.height() / 2.0) as i32).to_string(),
        ]
    };
    let [qx, qy] = location(&q);
    let [ex, ey] = location(&e);
    let xid = super::desktop::x11_test_focus(window.upcast_ref()).to_string();
    for active in [true, false] {
        let mut input = std::process::Command::new("/usr/bin/xdotool")
            .args([
                "mousemove",
                "--window",
                &xid,
                &qx,
                &qy,
                "mousedown",
                "1",
                "sleep",
                "0.06",
                "mousemove",
                "--window",
                &xid,
                &ex,
                &ey,
                "sleep",
                "0.06",
                "mouseup",
                "1",
            ])
            .spawn()
            .unwrap();
        pump(Duration::from_millis(300));
        assert!(input.wait().unwrap().success());
        assert_eq!([q.is_active(), w.is_active(), e.is_active()], [active; 3]);
    }
}

fn verify_background_does_not_freeze_controls(
    view: &super::View,
    window: &gtk::ApplicationWindow,
    spinner: &gtk::Spinner,
) {
    let received = Rc::new(Cell::new(false));
    let done = received.clone();
    assert!(view.tasks.background_keyboard(
        |_| -> std::io::Result<()> { unreachable!("fixture has no live session") },
        move |result| {
            assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::NotConnected);
            done.set(true);
        },
    ));
    assert!(view.pages.is_sensitive());
    assert!(window.is_sensitive());
    assert!(!spinner.is_spinning());
    wait_live("background fixture callback", || received.get());

    let delivered = received.clone();
    received.set(false);
    assert!(
        view.tasks
            .background_keyboard(|_| Ok(()), move |_| delivered.set(true),)
    );
    view.tasks.revision.set(view.tasks.revision.get() + 1);
    pump(Duration::from_millis(100));
    assert!(
        !received.get(),
        "an old session must not update the widgets"
    );
}

fn pump(duration: Duration) {
    let start = Instant::now();
    let context = glib::MainContext::default();
    while start.elapsed() < duration {
        while context.pending() {
            context.iteration(false);
        }
        std::thread::sleep(Duration::from_millis(4));
    }
}

fn find_label(root: &impl IsA<gtk::Widget>, text: &str) -> Option<gtk::Label> {
    find(root, |label: &gtk::Label| label.text() == text)
}

fn find_button(root: &impl IsA<gtk::Widget>, text: &str) -> Option<gtk::Button> {
    find(root, |button: &gtk::Button| {
        button.label().as_deref() == Some(text)
    })
}

fn find<T: glib::object::IsA<gtk::Widget> + glib::types::StaticType>(
    root: &impl IsA<gtk::Widget>,
    predicate: impl Fn(&T) -> bool,
) -> Option<T> {
    let mut pending = vec![root.clone().upcast::<gtk::Widget>()];
    while let Some(widget) = pending.pop() {
        if let Ok(item) = widget.clone().downcast::<T>()
            && predicate(&item)
        {
            return Some(item);
        }
        let mut child = widget.first_child();
        while let Some(current) = child {
            child = current.next_sibling();
            pending.push(current);
        }
    }
    None
}

fn screenshot(window: &impl IsA<gtk::Window>, name: &str) {
    let window = window.upcast_ref::<gtk::Window>();
    let Some(directory) = std::env::var_os("DRAGON_DRIVER_TEST_SCREENSHOTS") else {
        return;
    };
    let (width, height) = (window.width(), window.height());
    let paintable = gtk::WidgetPaintable::new(Some(window));
    let snapshot = gtk::Snapshot::new();
    paintable.snapshot(&snapshot, width.into(), height.into());
    let node = snapshot.to_node().expect("window must have painted");
    let surface = window.surface().unwrap();
    let renderer = gsk::Renderer::for_surface(&surface).unwrap();
    let bounds = graphene::Rect::new(0.0, 0.0, width as f32, height as f32);
    renderer
        .render_texture(&node, Some(&bounds))
        .save_to_png(std::path::Path::new(&directory).join(format!("{name}.png")))
        .unwrap();
    renderer.unrealize();
}

fn test_x11_clipboard() {
    let text = "Offline clipboard λ";
    let editor = gtk::TextView::new();
    let window = gtk::Window::builder()
        .title("Dragon Driver offline paste fixture")
        .child(&editor)
        .default_width(340)
        .default_height(140)
        .build();
    window.present();
    pump(Duration::from_millis(180));
    assert!(editor.grab_focus());
    let xid = super::desktop::x11_test_focus(&window);
    let mut desktop = super::desktop::Desktop::new(true);
    desktop.watch(true).unwrap();
    let super::desktop::Event::Focus(Some(focus)) = desktop.poll(Instant::now()).unwrap() else {
        panic!("expected foreground window");
    };
    assert_eq!(focus.window, xid);
    assert_eq!(
        focus.app_id.as_deref(),
        Some("org.dragondriver.OfflineFixture")
    );
    assert!(desktop.still_focused(xid));
    assert!(!desktop.still_focused(xid.wrapping_add(1)));
    let target = desktop.paste_target().expect("focus on Xvfb window");
    assert!(desktop.paste(target.wrapping_add(1)).is_err());
    let display = gtk::gdk::Display::default().unwrap();
    display.clipboard().set_text(text);
    display.flush();
    desktop.paste(target).unwrap();
    pump(Duration::from_millis(230));
    let buffer = editor.buffer();
    assert_eq!(
        buffer.text(&buffer.start_iter(), &buffer.end_iter(), false),
        text
    );
    window.close();
}

#[test]
#[ignore = "requires explicit opt-in and the physical wired K617M"]
fn wired_k617m_read_only() {
    let Ok(node) = std::env::var("DRAGON_DRIVER_LIVE_K617M") else {
        return;
    };
    gtk::init().expect("GTK must initialize");
    let app = gtk::Application::new(
        Some("org.dragondriver.DragonDriverLive"),
        gio::ApplicationFlags::NON_UNIQUE,
    );
    install_style(&app);
    app.register(gio::Cancellable::NONE).unwrap();
    let view = build_window(&app).unwrap();
    let window = view.window.upgrade().unwrap();
    view.scan();
    wait_live("USB discovery", || !view.tasks.busy.get());
    if view.connected.borrow().is_none() {
        let candidate = view
            .candidates
            .borrow()
            .iter()
            .position(|candidate| candidate.interface.node() == std::path::Path::new(&node))
            .expect("configuration interface must be discovered");
        view.connect(Some(candidate));
        wait_live("opening the K617M", || !view.tasks.busy.get());
    }
    {
        let connected = view.connected.borrow();
        let device = connected
            .as_ref()
            .expect("GTK must connect to the keyboard");
        assert_eq!(device.node, std::path::Path::new(&node));
        assert_eq!(
            (device.usb_id.vendor, device.usb_id.product),
            (0x0416, 0x7372)
        );
        assert_eq!(device.layout.family, "7153_AUSHERGB");
        assert_eq!(device.transport, Some(0));
        println!(
            "GTK connected to {} over USB · {} · firmware {}",
            device.name,
            device.node.display(),
            device.firmware
        );
    }
    assert!(view.pages.child_by_name("actuation").is_some());
    assert!(view.pages.child_by_name("profiles").is_some());
    assert!(find_label(&window, "Side lighting").is_none());
    assert!(find_label(&window, "Live 2.4 GHz lighting").is_none());
    assert!(find_button(&window, "Set gamepad mode").is_none());
    println!(
        "GTK showed Actuation and Profiles and hid side lighting, receiver lighting, and gamepad controls"
    );

    view.pages.set_visible_child_name("actuation");
    let page = view.pages.visible_child().unwrap();
    find_button(&view.keybed, "W")
        .unwrap()
        .downcast::<gtk::ToggleButton>()
        .unwrap()
        .set_active(true);
    wait_live("automatic key readback", || {
        find_label(&page, "Loaded from the keyboard").is_some()
    });
    let press = live_row::<gtk::Scale>(&page, "Press · mm");
    let separate = find(&page, |button: &gtk::CheckButton| {
        button.label().as_deref() == Some("Separate press and release")
    })
    .unwrap();
    let rapid = find(&page, |button: &gtk::CheckButton| {
        button.label().as_deref() == Some("Rapid Trigger")
    })
    .unwrap();
    println!(
        "GTK W (2:2) · press {:.1} mm · limit {:.1} mm · separate {} · Rapid Trigger {}",
        press.value(),
        press.adjustment().upper(),
        separate.is_active(),
        rapid.is_active()
    );
    pump(Duration::from_millis(200));
    screenshot(&window, "k617m-live-actuation");
    let key = |caption| {
        find_button(&view.keybed, caption)
            .unwrap()
            .downcast::<gtk::ToggleButton>()
            .unwrap()
    };
    key("W").set_active(false);
    key("A").set_active(true);
    wait_live("automatic readback for A", || {
        find_label(&page, "Loaded from the keyboard").is_some()
    });

    view.pages.set_visible_child_name("lighting");
    let page = view.pages.visible_child().unwrap();
    let effect = live_row::<gtk::ComboBoxText>(&page, "Effect");
    wait_live("automatic lighting readback", || {
        effect.active_id().is_some()
    });
    let modes = effect.model().unwrap().iter_n_children(None);
    let brightness = find(&view.keybed, |slider: &gtk::Scale| {
        slider.tooltip_text().as_deref() == Some("Keyboard brightness")
    })
    .unwrap()
    .value();
    assert!(effect.is_sensitive());
    assert!(modes > 0);
    println!(
        "GTK lighting · {} · {} modes · brightness {brightness}",
        effect.active_text().unwrap(),
        modes
    );
    pump(Duration::from_millis(200));
    screenshot(&window, "k617m-live-lighting");

    view.pages.set_visible_child_name("device");
    let page = view.pages.visible_child().unwrap();
    wait_live("automatic connection readback", || {
        find_label(&page, "Connected over USB").is_some()
    });
    println!("GTK Device status confirmed wired USB");
    let idle = find_label(&page, "Idle timers").unwrap().parent().unwrap();
    wait_live("automatic timers readback", || {
        find_button(&idle, "Apply timers").unwrap().is_sensitive()
    });
    assert!(
        find_button(&idle, "Apply timers").unwrap().is_sensitive(),
        "idle timers must load before Apply is enabled"
    );
    println!(
        "GTK idle timers · lighting {} s · keyboard {} s",
        live_row::<gtk::SpinButton>(&idle, "Turn off lighting after · seconds").value_as_int(),
        live_row::<gtk::SpinButton>(&idle, "Put keyboard to sleep after · seconds").value_as_int()
    );
    for caption in ["Anti-shake level", "Return rate"] {
        let row = find_label(&page, caption).unwrap().parent().unwrap();
        wait_live(caption, || {
            find_button(&row, "Apply").unwrap().is_sensitive()
        });
        assert!(
            find_button(&row, "Apply").unwrap().is_sensitive(),
            "{caption} must load before Apply is enabled"
        );
        if caption == "Return rate" {
            let value = find::<gtk::ComboBoxText>(&row, |_| true)
                .unwrap()
                .active_text();
            println!(
                "GTK {caption} · {}",
                value.as_deref().unwrap_or("unrecognized value")
            );
        } else {
            println!(
                "GTK {caption} · {}",
                find::<gtk::SpinButton>(&row, |_| true)
                    .unwrap()
                    .value_as_int()
            );
        }
    }
    window.close();
}

fn wait_live(operation: &str, done: impl Fn() -> bool) {
    let start = Instant::now();
    while !done() && start.elapsed() < Duration::from_secs(8) {
        pump(Duration::from_millis(20));
    }
    assert!(done(), "{operation} did not finish");
}

fn live_row<T: glib::object::IsA<gtk::Widget>>(root: &impl IsA<gtk::Widget>, label: &str) -> T {
    let row = find_label(root, label)
        .expect("field label")
        .parent()
        .unwrap();
    find(&row, |_| true).expect("field widget")
}
