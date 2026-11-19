pub const SDK_REPORT_LEN: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReportId {
    Default,
    Collection06,
}

impl ReportId {
    pub(crate) const fn byte(self) -> u8 {
        match self {
            Self::Default => 1,
            Self::Collection06 => 6,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameError {
    UnsupportedCommand,
    InvalidByte5,
}

pub fn frame_write_report(
    mut report: [u8; SDK_REPORT_LEN],
    report_id: ReportId,
) -> Result<[u8; SDK_REPORT_LEN], FrameError> {
    let command = match report[1] {
        value @ 0x00..=0x1b => value + 1,
        value @ 0x1c..=0x25 => value + 0x24,
        value @ 0x26..=0x2c => value - 8,
        _ => return Err(FrameError::UnsupportedCommand),
    };
    if report[5] >= 0x3b && command != 0x20 {
        return Err(FrameError::InvalidByte5);
    }

    report[0] = report_id.byte();
    report[1] = command;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_dll_boundaries_and_preserves_other_bytes() {
        for (source, expected) in [
            (0x00, 0x01),
            (0x1b, 0x1c),
            (0x1c, 0x40),
            (0x25, 0x49),
            (0x26, 0x1e),
            (0x2c, 0x24),
        ] {
            let mut input = [0x37; SDK_REPORT_LEN];
            input[1] = source;
            let mut output = input;
            output[0] = 6;
            output[1] = expected;
            assert_eq!(
                frame_write_report(input, ReportId::Collection06),
                Ok(output)
            );
        }
    }

    #[test]
    fn checks_byte_five_except_for_mapped_command_twenty() {
        let mut input = [0; SDK_REPORT_LEN];
        input[5] = 0x3b;
        assert_eq!(
            frame_write_report(input, ReportId::Default),
            Err(FrameError::InvalidByte5)
        );

        input[1] = 0x28;
        let frame = frame_write_report(input, ReportId::Default).unwrap();
        assert_eq!((frame[0], frame[1], frame[5]), (1, 0x20, 0x3b));

        input[1] = 0x2d;
        assert_eq!(
            frame_write_report(input, ReportId::Default),
            Err(FrameError::UnsupportedCommand)
        );
    }
}
