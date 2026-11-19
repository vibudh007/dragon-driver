use std::io::{self, Cursor};

use crate::keyboard::{lighting::RgbFrame, models::Position};
use fast_image_resize::{FilterType, ResizeAlg, ResizeOptions, Resizer};
use image::{DynamicImage, ImageFormat, ImageReader, Limits, RgbaImage};

// 29.2 cm × 10.2 cm at twenty samples per centimetre.
const WIDTH: u32 = 584;
const HEIGHT: u32 = 204;
const MAX_DECODE_BYTES: u64 = 160 * 1024 * 1024;

pub(crate) fn frame(bytes: &[u8], keys: &[(Position, [i32; 4])]) -> io::Result<RgbFrame> {
    sample(&resized(bytes, WIDTH, HEIGHT)?, keys)
}

pub(crate) fn resized(bytes: &[u8], width: u32, height: u32) -> io::Result<RgbaImage> {
    let mut reader = ImageReader::new(Cursor::new(bytes)).with_guessed_format()?;
    if !matches!(
        reader.format(),
        Some(ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP)
    ) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Use a PNG, JPEG or WebP wallpaper",
        ));
    }
    let mut limits = Limits::default();
    limits.max_image_width = Some(8192);
    limits.max_image_height = Some(8192);
    limits.max_alloc = Some(MAX_DECODE_BYTES);
    reader.limits(limits);
    let image = reader.decode().map_err(io::Error::other)?;
    let mut resized = DynamicImage::new(width, height, image.color());
    resized
        .set_color_space(image.color_space())
        .map_err(io::Error::other)?;
    // Keep the original channel-space triangle filter. Sampling composites alpha on black.
    let options = ResizeOptions::new()
        .resize_alg(ResizeAlg::Convolution(FilterType::Bilinear))
        .use_alpha(false);
    Resizer::new()
        .resize(&image, &mut resized, &options)
        .map_err(io::Error::other)?;
    Ok(resized.into_rgba8())
}

fn sample(image: &RgbaImage, keys: &[(Position, [i32; 4])]) -> io::Result<RgbFrame> {
    let bounds = keys
        .iter()
        .filter(|(_, [_, _, w, h])| *w > 0 && *h > 0)
        .map(|(_, [x, y, w, h])| [*x, *y, x + w, y + h])
        .reduce(|[left, top, right, bottom], [x0, y0, x1, y1]| {
            [left.min(x0), top.min(y0), right.max(x1), bottom.max(y1)]
        })
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::Unsupported, "Keyboard has no key geometry")
        })?;
    let [left, top, right, bottom] = bounds;
    let sx = f64::from(image.width()) / f64::from(right - left);
    let sy = f64::from(image.height()) / f64::from(bottom - top);
    let mut output = RgbFrame::default();
    for (position, [x, y, w, h]) in keys
        .iter()
        .copied()
        .filter(|(_, [_, _, w, h])| *w > 0 && *h > 0)
    {
        let x0 = f64::from(x - left) * sx;
        let y0 = f64::from(y - top) * sy;
        let x1 = f64::from(x + w - left) * sx;
        let y1 = f64::from(y + h - top) * sy;
        output[usize::from(position.row())][usize::from(position.column())] =
            enhance(mean_color(image, [x0, y0, x1, y1]));
    }
    Ok(output)
}

// Wallpaper Aura's HSV saturation and smooth contrast curve, applied after averaging.
fn enhance(rgb: [u8; 3]) -> [u8; 3] {
    let [r, g, b] = rgb;
    let peak = f64::from(r.max(g).max(b));
    let floor = f64::from(r.min(g).min(b));
    let black = 0.06 * 255.0;
    let white = 0.90 * 255.0;
    let t = ((peak - black) / (white - black)).clamp(0.0, 1.0);
    let value = 255.0 * t * t * (3.0 - 2.0 * t);
    if value <= 1.0 {
        return [1; 3];
    }
    let chroma = peak - floor;
    if chroma == 0.0 {
        return [value.round() as u8; 3];
    }
    let amplified_chroma = value * (2.0 * chroma / peak).min(1.0);
    rgb.map(|channel| {
        (value - amplified_chroma * ((peak - f64::from(channel)) / chroma)).round() as u8
    })
}

fn mean_color(image: &RgbaImage, [left, top, right, bottom]: [f64; 4]) -> [u8; 3] {
    let radius = (right - left).min(bottom - top) * 0.14;
    let mut sums = [0_u64; 3];
    let mut count = 0_u64;
    for y in top.floor() as u32..(bottom.ceil() as u32).min(image.height()) {
        let y_edge = (f64::from(y) + 0.5 - top).min(bottom - f64::from(y) - 0.5);
        for x in left.floor() as u32..(right.ceil() as u32).min(image.width()) {
            let x_edge = (f64::from(x) + 0.5 - left).min(right - f64::from(x) - 0.5);
            if x_edge < 0.0
                || y_edge < 0.0
                || (radius - x_edge).max(0.0).hypot((radius - y_edge).max(0.0)) > radius
            {
                continue;
            }
            // Reverse the vertical axis while leaving the horizontal position intact.
            let [r, g, b, alpha] = image.get_pixel(x, image.height() - 1 - y).0;
            for (sum, value) in sums.iter_mut().zip([r, g, b]) {
                *sum += u64::from(value) * u64::from(alpha);
            }
            count += 1;
        }
    }
    if count == 0 {
        return [0; 3];
    }
    let denominator = count * 255;
    sums.map(|sum| ((sum + denominator / 2) / denominator) as u8)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn wallpaper_aura_curve_preserves_neutrals_and_boosts_color() {
        assert_eq!(enhance([199, 106, 93]), [241, 30, 0]);
        assert_eq!(enhance([14, 1, 7]), [1; 3]);
        assert_eq!(enhance([128; 3]), [137; 3]);
        assert_eq!(enhance([255; 3]), [255; 3]);
    }

    #[test]
    fn flipped_keys_keep_left_to_right_order_and_ignore_gaps() {
        let image = RgbaImage::from_fn(WIDTH, HEIGHT, |x, y| {
            Rgba(match (x < WIDTH / 2, y < HEIGHT / 2) {
                (true, true) => [255, 0, 0, 255],
                (false, true) => [0, 255, 0, 255],
                (true, false) => [0, 0, 255, 255],
                (false, false) => [255, 255, 0, 255],
            })
        });
        let keys = [
            (Position::new(0, 0).unwrap(), [0, 0, 48, 48]),
            (Position::new(0, 2).unwrap(), [52, 0, 48, 48]),
            (Position::new(1, 0).unwrap(), [0, 52, 48, 48]),
            (Position::new(1, 2).unwrap(), [52, 52, 48, 48]),
        ];
        let result = sample(&image, &keys).unwrap();
        assert_eq!(result[0][0], [0, 0, 255]);
        assert_eq!(result[0][2], [255, 255, 0]);
        assert_eq!(result[1][0], [255, 0, 0]);
        assert_eq!(result[1][2], [0, 255, 0]);
        assert_eq!(result[0][1], [0; 3]);
    }

    #[test]
    fn rectangular_reshape_preserves_both_ends_of_a_tall_image() {
        let image = RgbaImage::from_fn(4, 48, |_, y| {
            if y < 8 {
                Rgba([255, 0, 0, 255])
            } else if y >= 40 {
                Rgba([0, 0, 255, 255])
            } else {
                Rgba([0, 220, 0, 255])
            }
        });
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .unwrap();
        let keys = [
            (Position::new(0, 0).unwrap(), [0, 0, 100, 10]),
            (Position::new(1, 0).unwrap(), [0, 90, 100, 10]),
        ];
        let result = frame(&bytes, &keys).unwrap();
        assert_eq!(result[0][0], [0, 0, 255]);
        assert_eq!(result[1][0], [255, 0, 0]);
    }

    #[test]
    fn accelerated_resize_matches_triangle_filter_for_transparent_images() {
        let image = RgbaImage::from_fn(613, 251, |x, y| {
            Rgba([x as u8, y as u8, (x + y) as u8, (x * y) as u8])
        });
        let original =
            image::imageops::resize(&image, WIDTH, HEIGHT, image::imageops::FilterType::Triangle);
        let keys = [
            (Position::new(0, 0).unwrap(), [0, 0, 27, 25]),
            (Position::new(0, 1).unwrap(), [30, 0, 15, 25]),
            (Position::new(1, 0).unwrap(), [5, 28, 40, 25]),
        ];
        let mut bytes = Vec::new();
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::WebP)
            .unwrap();
        let expected = sample(&original, &keys).unwrap();
        let actual = frame(&bytes, &keys).unwrap();
        for (left, right) in actual
            .iter()
            .flatten()
            .flatten()
            .zip(expected.iter().flatten().flatten())
        {
            assert!(left.abs_diff(*right) <= 1);
        }
    }
}
