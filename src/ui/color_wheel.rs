use std::cell::{Cell, RefCell};
use std::f64::consts::TAU;
use std::rc::Rc;

use gtk::{Orientation, cairo, prelude::*};

type Rgb = [u8; 3];
type ColorSet = Box<dyn Fn(Rgb)>;
const SIZE: i32 = 208;
const SWATCH_WIDTH: i32 = 38;
const SWATCH_HEIGHT: i32 = 28;
const DEFAULT_RED: Rgb = [255, 0, 0];

thread_local! {
    static WHEEL_SURFACE: Option<cairo::ImageSurface> = wheel_surface();
    static RAINBOW_SURFACE: Option<cairo::ImageSurface> = rainbow_surface();
}

#[derive(Clone)]
pub(super) struct ColorWheel {
    button: gtk::Button,
    swatch: gtk::DrawingArea,
    color: Rc<Cell<Rgb>>,
    changed: Rc<RefCell<Option<ColorSet>>>,
}

impl ColorWheel {
    pub fn new(title: &str) -> Self {
        let initial = DEFAULT_RED;
        let button = gtk::Button::new();
        button.add_css_class("color-wheel-button");
        button.set_valign(gtk::Align::Center);
        button.set_tooltip_text(Some(title));
        button.update_property(&[gtk::accessible::Property::Label(title)]);
        let swatch = swatch_area();
        let color = Rc::new(Cell::new(initial));
        let displayed = color.clone();
        swatch.set_draw_func(move |_, cr, width, height| {
            swatch_path(cr, width, height);
            let [r, g, b] = displayed.get().map(|value| f64::from(value) / 255.0);
            cr.set_source_rgb(r, g, b);
            let _ = cr.fill();
        });
        button.set_child(Some(&swatch));

        let dialog = gtk::Window::builder()
            .title(title)
            .modal(true)
            .resizable(false)
            .hide_on_close(true)
            .destroy_with_parent(true)
            .build();
        let window = dialog.clone();
        button.connect_clicked(move |button| {
            if let Some(parent) = button.root().and_downcast::<gtk::Window>() {
                window.set_transient_for(Some(&parent));
                window.present();
            }
        });
        let owned = dialog.clone();
        button.connect_destroy(move |_| owned.destroy());
        let panel = gtk::Box::new(Orientation::Vertical, 8);
        panel.add_css_class("wheel-panel");
        let drawing = gtk::DrawingArea::builder()
            .content_width(SIZE)
            .content_height(SIZE)
            .halign(gtk::Align::Center)
            .build();
        panel.append(&drawing);
        let value = gtk::Scale::with_range(Orientation::Horizontal, 0.0, 100.0, 1.0);
        value.set_draw_value(false);
        value.set_size_request(110, -1);
        panel.append(&super::field("Color brightness", &value));
        let entry = gtk::Entry::builder()
            .text(format_hex(initial))
            .max_length(7)
            .width_chars(8)
            .build();
        panel.append(&super::field("Hex color", &entry));
        let done = gtk::Button::with_label("Done");
        done.set_halign(gtk::Align::End);
        panel.append(&done);
        dialog.set_child(Some(&panel));
        let window = dialog.downgrade();
        done.connect_clicked(move |_| {
            if let Some(window) = window.upgrade() {
                window.close();
            }
        });
        let key = gtk::EventControllerKey::new();
        let window = dialog.downgrade();
        key.connect_key_pressed(move |_, key, _, _| {
            if key == gtk::gdk::Key::Escape
                && let Some(window) = window.upgrade()
            {
                window.close();
                return gtk::glib::Propagation::Stop;
            }
            gtk::glib::Propagation::Proceed
        });
        dialog.add_controller(key);

        let hsv = Rc::new(Cell::new(rgb_to_hsv(initial)));
        let following = Rc::new(Cell::new(true));
        if let Some(surface) = WHEEL_SURFACE.with(Clone::clone) {
            let marker = hsv.clone();
            drawing.set_draw_func(move |_, cr, width, height| {
                let center = f64::from(SIZE) / 2.0;
                let radius = center - 3.0;
                let [hue, saturation, _] = marker.get();
                // Widgets keep the wheel at its natural size. Center it if the theme adds space.
                cr.translate(
                    (f64::from(width) - f64::from(SIZE)) / 2.0,
                    (f64::from(height) - f64::from(SIZE)) / 2.0,
                );
                if cr.set_source_surface(&surface, 0.0, 0.0).is_ok() {
                    let _ = cr.paint();
                }
                cr.arc(
                    center + hue.cos() * saturation * radius,
                    center + hue.sin() * saturation * radius,
                    5.0,
                    0.0,
                    TAU,
                );
                cr.set_line_width(3.5);
                cr.set_source_rgb(0.0, 0.0, 0.0);
                let _ = cr.stroke_preserve();
                cr.set_line_width(1.5);
                cr.set_source_rgb(1.0, 1.0, 1.0);
                let _ = cr.stroke();
            });
        }

        let syncing = Rc::new(Cell::new(false));
        let sync = {
            let syncing = syncing.clone();
            let (entry, value, drawing) =
                (entry.downgrade(), value.downgrade(), drawing.downgrade());
            let hsv = hsv.clone();
            move || {
                if let (Some(entry), Some(value), Some(drawing)) =
                    (entry.upgrade(), value.upgrade(), drawing.upgrade())
                {
                    let draft = hsv.get();
                    syncing.set(true);
                    entry.set_text(&format_hex(hsv_to_rgb(draft)));
                    value.set_value(draft[2] * 100.0);
                    syncing.set(false);
                    drawing.queue_draw();
                }
            }
        };
        let reset = sync.clone();
        let (shown, committed) = (hsv.clone(), color.clone());
        let reset_following = following.clone();
        dialog.connect_show(move |_| {
            reset_following.set(true);
            shown.set(rgb_to_hsv(committed.get()));
            reset();
        });

        let changed = Rc::new(RefCell::new(None::<ColorSet>));
        let apply = {
            let committed = color.clone();
            let changed = changed.clone();
            let swatch = swatch.clone();
            move |color| {
                committed.set(color);
                swatch.queue_draw();
                if let Some(callback) = changed.borrow().as_ref() {
                    callback(color);
                }
            }
        };
        let draft = hsv.clone();
        let synchronizing = syncing.clone();
        let brightness = value.downgrade();
        let marker = drawing.downgrade();
        let apply_hex = apply.clone();
        entry.connect_changed(move |entry| {
            let parsed = parse_hex(&entry.text());
            if parsed.is_some() {
                entry.remove_css_class("error");
            } else {
                entry.add_css_class("error");
            }
            if !synchronizing.get()
                && let Some(color) = parsed
            {
                let current = rgb_to_hsv(color);
                draft.set(current);
                synchronizing.set(true);
                if let Some(brightness) = brightness.upgrade() {
                    brightness.set_value(current[2] * 100.0);
                }
                synchronizing.set(false);
                if let Some(drawing) = marker.upgrade() {
                    drawing.queue_draw();
                }
                apply_hex(color);
            }
        });
        let draft = hsv.clone();
        let synchronizing = syncing.clone();
        let refresh = sync.clone();
        let apply_brightness = apply.clone();
        value.connect_value_changed(move |value| {
            if !synchronizing.get() {
                let mut current = draft.get();
                current[2] = value.value() / 100.0;
                draft.set(current);
                refresh();
                apply_brightness(hsv_to_rgb(current));
            }
        });
        let weak = drawing.downgrade();
        let committed = color.clone();
        let pick = move |x: f64, y: f64| {
            let Some(drawing) = weak.upgrade() else {
                return false;
            };
            let dx = x - f64::from(drawing.width()) / 2.0;
            let dy = y - f64::from(drawing.height()) / 2.0;
            let saturation = dx.hypot(dy) / (f64::from(SIZE) / 2.0 - 3.0);
            if saturation > 1.0 {
                return false;
            }
            let [_, _, brightness] = hsv.get();
            let draft = [
                dy.atan2(dx).rem_euclid(TAU),
                saturation,
                if brightness == 0.0 { 1.0 } else { brightness },
            ];
            hsv.set(draft);
            let picked = hsv_to_rgb(draft);
            sync();
            if picked != committed.get() {
                apply(picked);
            }
            true
        };
        drawing.set_tooltip_text(Some(
            "Click to pin the color. Click again to follow the pointer.",
        ));
        let motion = gtk::EventControllerMotion::new();
        let follow = following.clone();
        let hover = pick.clone();
        let track = move |x, y| {
            if follow.get() {
                hover(x, y);
            }
        };
        let enter = track.clone();
        motion.connect_enter(move |_, x, y| enter(x, y));
        motion.connect_motion(move |_, x, y| track(x, y));
        drawing.add_controller(motion);
        let click = gtk::GestureClick::new();
        click.set_button(1);
        click.connect_released(move |_, _, x, y| {
            if pick(x, y) {
                following.set(!following.get());
                super::lighting_trace::event(format_args!("wheel following={}", following.get()));
            }
        });
        drawing.add_controller(click);

        Self {
            button,
            swatch,
            color,
            changed,
        }
    }

    pub fn widget(&self) -> &gtk::Button {
        &self.button
    }
    pub fn rgb(&self) -> Rgb {
        self.color.get()
    }

    pub fn set_rgb(&self, value: Rgb) {
        self.color.set(value);
        self.swatch.queue_draw();
    }

    pub fn connect_color_set(&self, changed: impl Fn(Rgb) + 'static) {
        *self.changed.borrow_mut() = Some(Box::new(changed));
    }
}

fn swatch_area() -> gtk::DrawingArea {
    gtk::DrawingArea::builder()
        .content_width(SWATCH_WIDTH)
        .content_height(SWATCH_HEIGHT)
        .build()
}

fn swatch_path(cr: &cairo::Context, width: i32, height: i32) {
    let (width, height) = (f64::from(width), f64::from(height));
    let inset = 1.0;
    let radius = 7.0_f64.min((width.min(height) / 2.0 - inset).max(0.0));
    let near = inset + radius;
    let far_x = width - near;
    let far_y = height - near;
    let quarter = TAU / 4.0;
    for (x, y, start) in [
        (far_x, near, -quarter),
        (far_x, far_y, 0.0),
        (near, far_y, quarter),
        (near, near, 2.0 * quarter),
    ] {
        cr.arc(x, y, radius, start, start + quarter);
    }
    cr.close_path();
}

pub(super) fn rainbow_button() -> gtk::ToggleButton {
    let button = gtk::ToggleButton::new();
    button.add_css_class("color-wheel-button");
    button.add_css_class("rainbow-swatch");
    button.set_valign(gtk::Align::Center);
    button.set_tooltip_text(Some("Rainbow"));
    button.update_property(&[gtk::accessible::Property::Label("Rainbow")]);
    let swatch = swatch_area();
    swatch.set_draw_func(|_, cr, width, height| {
        RAINBOW_SURFACE.with(|surface| {
            if let Some(surface) = surface {
                swatch_path(cr, width, height);
                cr.clip();
                cr.scale(
                    f64::from(width) / f64::from(surface.width()),
                    f64::from(height) / f64::from(surface.height()),
                );
                if cr.set_source_surface(surface, 0.0, 0.0).is_ok() {
                    let _ = cr.paint();
                }
            }
        });
    });
    button.set_child(Some(&swatch));
    button
}

fn rainbow_surface() -> Option<cairo::ImageSurface> {
    let (width, height) = (SWATCH_WIDTH * 2, SWATCH_HEIGHT * 2);
    let mut surface = cairo::ImageSurface::create(cairo::Format::Rgb24, width, height).ok()?;
    let stride = surface.stride() as usize;
    {
        let mut data = surface.data().ok()?;
        for (y, row) in data.chunks_exact_mut(stride).enumerate() {
            for (x, pixel) in row[..width as usize * 4].chunks_exact_mut(4).enumerate() {
                let dx = (x as f64 + 0.5) / f64::from(width) * 2.0 - 1.0;
                let dy = (y as f64 + 0.5) / f64::from(height) * 2.0 - 1.0;
                let radius = dx.hypot(dy);
                let hue = (dy.atan2(dx) + radius * 0.65).rem_euclid(TAU);
                let sector = hue / TAU * 8.0;
                let blend = ((sector.fract() - 0.36) / 0.28).clamp(0.0, 1.0);
                let blend = blend * blend * (3.0 - 2.0 * blend);
                let saturation = (radius * 5.0).clamp(0.0, 0.86);
                let [r, g, b] =
                    hsv_to_rgb([(sector.floor() + blend) * TAU / 8.0, saturation, 0.98]);
                pixel.copy_from_slice(&u32::from_be_bytes([0, r, g, b]).to_ne_bytes());
            }
        }
    }
    Some(surface)
}

fn format_hex([r, g, b]: Rgb) -> String {
    format!("#{r:02X}{g:02X}{b:02X}")
}

fn parse_hex(input: &str) -> Option<Rgb> {
    let input = input.strip_prefix('#').unwrap_or(input);
    if input.len() != 6 || !input.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let color = u32::from_str_radix(input, 16).ok()?;
    let [_, r, g, b] = color.to_be_bytes();
    Some([r, g, b])
}

fn rgb_to_hsv(color: Rgb) -> [f64; 3] {
    let [r, g, b] = color.map(|value| f64::from(value) / 255.0);
    let value = r.max(g).max(b);
    let range = value - r.min(g).min(b);
    if range == 0.0 {
        return [0.0, 0.0, value];
    }
    let sector = if r == value {
        (g - b) / range
    } else if g == value {
        2.0 + (b - r) / range
    } else {
        4.0 + (r - g) / range
    };
    [sector.rem_euclid(6.0) * TAU / 6.0, range / value, value]
}

fn hsv_to_rgb([hue, saturation, value]: [f64; 3]) -> Rgb {
    let sector = hue.rem_euclid(TAU) * 6.0 / TAU;
    let chroma = value * saturation;
    let middle = chroma * (1.0 - (sector.rem_euclid(2.0) - 1.0).abs());
    let rgb = match sector as u8 {
        0 => [chroma, middle, 0.0],
        1 => [middle, chroma, 0.0],
        2 => [0.0, chroma, middle],
        3 => [0.0, middle, chroma],
        4 => [middle, 0.0, chroma],
        _ => [chroma, 0.0, middle],
    };
    rgb.map(|channel| ((channel + value - chroma) * 255.0).round() as u8)
}

fn wheel_surface() -> Option<cairo::ImageSurface> {
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, SIZE, SIZE).ok()?;
    let stride = surface.stride() as usize;
    let center = f64::from(SIZE) / 2.0;
    let radius = center - 3.0;
    {
        let mut pixels = surface.data().ok()?;
        for (y, row) in pixels.chunks_exact_mut(stride).enumerate() {
            for (x, pixel) in row[..SIZE as usize * 4].chunks_exact_mut(4).enumerate() {
                let (dx, dy) = (x as f64 + 0.5 - center, y as f64 + 0.5 - center);
                let distance = dx.hypot(dy);
                let alpha = (radius + 0.5 - distance).clamp(0.0, 1.0);
                if alpha != 0.0 {
                    let color = hsv_to_rgb([dy.atan2(dx), (distance / radius).min(1.0), 1.0]);
                    let [r, g, b] = color.map(|value| (alpha * f64::from(value)).round() as u8);
                    let packed = u32::from_be_bytes([(alpha * 255.0).round() as u8, r, g, b]);
                    pixel.copy_from_slice(&packed.to_ne_bytes());
                }
            }
        }
    }
    Some(surface)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn precise_colors_round_trip() {
        for color in [
            [0, 0, 0],
            [255; 3],
            [238, 130, 238],
            [0, 255, 37],
            [42, 42, 42],
            [1, 2, 3],
        ] {
            assert_eq!(hsv_to_rgb(rgb_to_hsv(color)), color);
            assert_eq!(parse_hex(&format_hex(color)), Some(color));
        }
        assert_eq!(parse_hex("💡ab"), None);
        assert_eq!(parse_hex("#ffee"), None);
    }

    #[test]
    fn swatch_mask_leaves_the_rounded_border_clear() {
        let mut surface =
            cairo::ImageSurface::create(cairo::Format::ARgb32, SWATCH_WIDTH, SWATCH_HEIGHT)
                .unwrap();
        {
            let context = cairo::Context::new(&surface).unwrap();
            swatch_path(&context, SWATCH_WIDTH, SWATCH_HEIGHT);
            context.set_source_rgb(1.0, 0.0, 0.0);
            context.fill().unwrap();
        }
        let (width, height) = (SWATCH_WIDTH as usize, SWATCH_HEIGHT as usize);
        let stride = surface.stride() as usize;
        let pixels = surface.data().unwrap();
        let alpha = |x, y| {
            let offset = y * stride + x * 4;
            u32::from_ne_bytes(pixels[offset..offset + 4].try_into().unwrap()) >> 24
        };
        for (x, y) in [
            (2, 2),
            (width - 3, 2),
            (2, height - 3),
            (width - 3, height - 3),
            (0, height / 2),
            (width / 2, 0),
        ] {
            assert_eq!(alpha(x, y), 0);
        }
        assert_eq!(alpha(width / 2, height / 2), 255);
    }
}
