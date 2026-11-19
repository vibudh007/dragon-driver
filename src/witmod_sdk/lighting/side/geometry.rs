use super::{MAX_SIDE_CELLS, SideEffect, SidePreview};
use crate::witmod_sdk::lighting::{PreviewCell, PreviewConfigError, mix_color};

const TABLE_BYTES: usize = 2 + MAX_SIDE_CELLS * 4;
const TABLES: &[u8; 20 * TABLE_BYTES] = include_bytes!("../side-waves.bin");

const SHORT_SPANS: &[&str] = &[
    "SD002JLBTRGB",
    "K235HERGB",
    "V75CZHERGB",
    "X86HERGB",
    "V75XHERGB",
    "X86MKHERGB",
    "MKGA75HERGB",
    "V75XYXNJDLHERGB",
    "X76JHXNHJHERGB",
    "V75proHERGB",
];

const MODELS: [(&[&str], usize); 20] = [
    (&["K665RGB"], 6),
    (&["K668RGB", "K171UKRGB", "K205UKRGB"], 6),
    (&["K665UKIRIBARGB", "KD1IR_MRGB", "Mixabl84RGB"], 18),
    (&["KM87RGB"], 26),
    (&["TY84HKRGB", "TY84DREVORGB"], 24),
    (&["EP-K87_MRGB"], 22),
    (&["GM810RGB", "GM810UKRGB"], 6),
    (&["K950RXHERGB"], 6),
    (&["K24RGB"], 13),
    (
        &[
            "K221CVRGB",
            "K221UKCVRGB",
            "K221JPCVRGB",
            "K221IRCVRGB",
            "K221IRCVUKRGB",
            "K221IRCVJPRGB",
        ],
        15,
    ),
    (&["Mek5"], 4),
    (&["K75RGB"], 14),
    (
        &[
            "Y75BTRGB",
            "Y84BTRGB",
            "Y75JLBTRGB",
            "K241JLBTRGB",
            "Y84JLBTRGB",
        ],
        23,
    ),
    (&["K30RGB"], 18),
    (&["MK108BTRGB"], 26),
    (&["VK68BTJL_RGB"], 11),
    (&["D09HERGB"], 6),
    (&["L67HE_RGB"], 8),
    (&["MK20HERGB"], 10),
    (&["GK6930GHERGB"], 10),
];

fn matches(model: &str, names: &[&str]) -> bool {
    names.iter().any(|name| name.eq_ignore_ascii_case(model))
}

fn wave_layout(model: &str) -> Option<(usize, usize)> {
    if matches(model, SHORT_SPANS) {
        return Some((10, 4));
    }
    MODELS
        .iter()
        .enumerate()
        .find_map(|(index, &(names, cutoff))| {
            matches(model, names).then_some((
                index,
                if model.eq_ignore_ascii_case("Mixabl84RGB") {
                    13
                } else {
                    cutoff
                },
            ))
        })
}

pub(in crate::witmod_sdk::lighting) fn collision_span(model: &str) -> u8 {
    for (names, span) in [
        (&["KR081BTRGB"][..], 26),
        (&["MK108BTRGB"][..], 51),
        (&["Y75JLBTRGB", "Y84JLBTRGB"][..], 45),
        (&["K241JLBTRGB"][..], 16),
        (SHORT_SPANS, 6),
        (&["VK68BTJL_RGB"][..], 21),
        (&["MK20HERGB", "GK6930GHERGB"][..], 18),
    ] {
        if matches(model, names) {
            return span;
        }
    }
    33
}

impl SidePreview {
    pub(in crate::witmod_sdk::lighting) fn wave_for_model(
        model: &str,
        slots: &[bool],
        foreground: [u8; 3],
        background: [u8; 3],
        reverse: bool,
        rainbow: bool,
    ) -> Result<Self, PreviewConfigError> {
        let Some((layout, cutoff)) = wave_layout(model) else {
            return Ok(Self::wave_default(
                slots.len(),
                foreground,
                background,
                rainbow,
            ));
        };
        let data = &TABLES[layout * TABLE_BYTES..][..TABLE_BYTES];
        let count = usize::from(data[usize::from(rainbow)]);
        if slots.iter().filter(|&&lit| lit).count() > count {
            return Err(PreviewConfigError::SideLayoutTooLong {
                maximum_active: count,
            });
        }

        let mut preview = Self::wave_default(slots.len(), foreground, background, rainbow);
        let mut position = 0;
        let mut falling = false;
        let mut phases = Vec::with_capacity(slots.len());
        for (index, (&lit, cell)) in slots.iter().zip(&mut preview.cells).enumerate() {
            if lit {
                if rainbow {
                    let start = 2 + MAX_SIDE_CELLS + position * 3;
                    cell.color.copy_from_slice(&data[start..start + 3]);
                } else {
                    let initial = data[2 + position];
                    let level = if reverse { 255 - initial } else { initial };
                    *cell = PreviewCell {
                        color: mix_color(foreground, background, level, false),
                        level,
                    };
                    falling = index < cutoff;
                }
                position += 1;
            }
            if !rainbow {
                phases.push(falling);
            }
        }
        preview.effect = if rainbow {
            SideEffect::Spectrum {
                forward: reverse,
                step: 9,
            }
        } else {
            SideEffect::Wave { falling: phases }
        };
        Ok(preview)
    }
}
