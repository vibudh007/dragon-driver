use super::hid::ReportId;

const CONFIG_USAGE: u32 = 0xff1b_0091;
const CONFIG_BITS: u32 = 63 * 8;

#[derive(Clone, Copy, Default)]
struct Global {
    page: u32,
    size: u32,
    count: u32,
    report: u8,
}

#[derive(Clone, Copy, Default)]
struct Report {
    input: u32,
    output: u32,
    incompatible: bool,
}

pub(super) fn configuration_report_id(mut descriptor: &[u8]) -> Option<ReportId> {
    let mut global = Global::default();
    let mut stack = Vec::new();
    let mut usage = None;
    let mut depth = 0_u16;
    let mut configuration = false;
    let mut reports = [Report::default(); 2];

    while let Some((&header, rest)) = descriptor.split_first() {
        // HID long items do not supply any of the report fields this parser needs.
        if header == 0xfe {
            let length = usize::from(*rest.first()?);
            descriptor = rest.get(length + 2..)?;
            continue;
        }
        let length = [0, 1, 2, 4][usize::from(header & 3)];
        let field = rest.get(..length)?;
        let mut value = [0; 4];
        value[..length].copy_from_slice(field);
        let value = u32::from_le_bytes(value);
        descriptor = rest.get(length..)?;

        match header & 0xfc {
            0x04 => global.page = u32::from(u16::try_from(value).ok()?),
            0x74 => global.size = value,
            0x84 => global.report = std::num::NonZeroU8::new(u8::try_from(value).ok()?)?.get(),
            0x94 => global.count = value,
            0xa4 => stack.push(global),
            0xb4 => global = stack.pop()?,
            0x08 => {
                usage.get_or_insert(if length == 4 {
                    value
                } else {
                    (global.page << 16) | value
                });
            }
            0xa0 => {
                if depth == 0 {
                    configuration = value == 1 && usage == Some(CONFIG_USAGE);
                }
                depth = depth.checked_add(1)?;
            }
            0xc0 => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    configuration = false;
                }
            }
            0x80 | 0x90 => {
                let index = match global.report {
                    1 => Some(0),
                    6 => Some(1),
                    _ => None,
                };
                if let Some(index) = index {
                    let report = &mut reports[index];
                    let bits = global.size.checked_mul(global.count)?;
                    let field_bits = if header & 0xfc == 0x80 {
                        &mut report.input
                    } else {
                        &mut report.output
                    };
                    *field_bits = field_bits.checked_add(bits)?;
                    if !configuration || value != 2 || global.size != 8 {
                        report.incompatible = true;
                    }
                }
            }
            _ => {}
        }
        // Local items apply to the next main item only.
        if header & 0x0c == 0 {
            usage = None;
        }
    }

    if depth != 0 || !stack.is_empty() {
        return None;
    }
    let [one, six] = reports.map(|report| {
        !report.incompatible && report.input == CONFIG_BITS && report.output == CONFIG_BITS
    });
    match (one, six) {
        (true, false) => Some(ReportId::Default),
        (false, true) => Some(ReportId::Collection06),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONFIG: [u8; 17] = [
        0x06, 0x1b, 0xff, 0x09, 0x91, 0xa1, 1, 0x85, 6, 0x75, 8, 0x95, 63, 0x81, 2, 0x91, 2,
    ];

    #[test]
    fn composite_descriptor_requires_a_unique_configuration_collection() {
        let mut descriptor = vec![
            0x05, 1, 0x09, 6, 0xa1, 1, 0x85, 1, 0x75, 8, 0x95, 8, 0x81, 2, 0xc0,
        ];
        descriptor.extend(CONFIG);
        descriptor.push(0xc0);
        assert_eq!(
            configuration_report_id(&descriptor),
            Some(ReportId::Collection06)
        );
        descriptor[7] = 6;
        assert_eq!(configuration_report_id(&descriptor), None);
        assert_eq!(configuration_report_id(&CONFIG), None);
        assert_eq!(configuration_report_id(&[0xfe]), None);
    }
}
