use crate::keyboard::lighting::AudioPalette;
use image::RgbaImage;

pub(crate) fn extract(bytes: &[u8]) -> std::io::Result<AudioPalette> {
    let image = super::wallpaper_colors::resized(bytes, 96, 64)?;
    Ok(prominent(&image))
}

#[derive(Clone, Copy, Default)]
struct Bucket {
    sum: [u32; 3],
    count: u32,
}

impl Bucket {
    fn add(&mut self, rgb: [u8; 3]) {
        self.count += 1;
        for (sum, channel) in self.sum.iter_mut().zip(rgb) {
            *sum += u32::from(channel);
        }
    }

    fn color(self) -> [u8; 3] {
        self.sum
            .map(|sum| ((sum + self.count / 2) / self.count) as u8)
    }
}

fn distance(a: [u8; 3], b: [u8; 3]) -> u32 {
    a.into_iter()
        .zip(b)
        .map(|(l, r)| u32::from(l.abs_diff(r)).pow(2))
        .sum()
}

fn prominent(image: &RgbaImage) -> AudioPalette {
    let mut histogram = vec![Bucket::default(); 4096];
    for pixel in image.pixels() {
        let [r, g, b, alpha] = pixel.0;
        let color =
            [r, g, b].map(|channel| ((u16::from(channel) * u16::from(alpha) + 127) / 255) as u8);
        let index = (usize::from(color[0] >> 4) << 8)
            | (usize::from(color[1] >> 4) << 4)
            | usize::from(color[2] >> 4);
        histogram[index].add(color);
    }
    let samples: Vec<_> = histogram
        .into_iter()
        .filter(|bucket| bucket.count != 0)
        .collect();
    let first = samples
        .iter()
        .max_by_key(|bucket| bucket.count)
        .map_or([0; 3], |b| b.color());
    let mut colors = [first; 4];
    for index in 1..colors.len() {
        colors[index] = samples
            .iter()
            .max_by_key(|bucket| {
                let color = bucket.color();
                let separation = colors[..index]
                    .iter()
                    .map(|&center| distance(center, color))
                    .min()
                    .unwrap_or(0);
                u64::from(bucket.count) * u64::from(separation)
            })
            .map_or(first, |b| b.color());
    }
    for _ in 0..5 {
        let mut groups = [Bucket::default(); 4];
        for sample in &samples {
            let color = sample.color();
            let closest = (0..4)
                .min_by_key(|&i| distance(colors[i], color))
                .unwrap_or(0);
            let group = &mut groups[closest];
            group.count += sample.count;
            for (sum, part) in group.sum.iter_mut().zip(sample.sum) {
                *sum += part;
            }
        }
        let updated = std::array::from_fn(|i| {
            if groups[i].count == 0 {
                colors[i]
            } else {
                groups[i].color()
            }
        });
        if updated == colors {
            break;
        }
        colors = updated;
    }
    colors.sort_by_key(|rgb| {
        let high = rgb[0].max(rgb[1]).max(rgb[2]);
        let low = rgb[0].min(rgb[1]).min(rgb[2]);
        (
            u32::from(high - low) * 65_535 / u32::from(high.max(1)),
            high,
        )
    });
    colors
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn ranks_four_dominant_colors_by_saturation() {
        let colors = [[120, 120, 120], [140, 105, 105], [170, 85, 85], [255, 0, 0]];
        let image = RgbaImage::from_fn(40, 20, |x, _| {
            let [r, g, b] = colors[(x / 10) as usize];
            Rgba([r, g, b, 255])
        });
        assert_eq!(prominent(&image), colors);
    }

    #[test]
    fn a_solid_wallpaper_produces_a_stable_palette() {
        assert_eq!(
            prominent(&RgbaImage::from_pixel(4, 4, Rgba([25, 44, 77, 255]))),
            [[25, 44, 77]; 4]
        );
    }
}
