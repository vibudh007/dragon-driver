use std::io;

use crate::witmod_sdk::profile::KeyAction;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DesktopBinding {
    Text(String),
    Launch(String),
    Profile(String),
}

impl DesktopBinding {
    pub(crate) fn from_assignment(action: KeyAction<'_>) -> io::Result<Option<Self>> {
        let value = action.value;
        let result = match action.operation_type {
            "6" if !value.is_empty() && value.len() <= 5119 => Self::Text(value.to_owned()),
            "7" if value.parse::<i32>().is_ok() => Self::Profile(value.to_owned()),
            "8" if !value.is_empty() && value.len() <= 5119 => Self::Launch(value.to_owned()),
            "6" | "7" | "8" => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "Invalid desktop assignment",
                ));
            }
            _ => return Ok(None),
        };
        Ok(Some(result))
    }
}

pub(crate) fn pressed_key(report: &[u8], report_id: u8) -> Option<u8> {
    if !report.starts_with(&[report_id, 0x20]) || *report.get(10)? == 0 {
        return None;
    }
    let code = report[9];
    (code != 0).then_some(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_a_host_binding_press_from_the_selected_collection() {
        let mut report = [1, 0x20, 0, 0, 0, 0, 0, 0, 0, 26, 1];
        assert_eq!(pressed_key(&report, 1), Some(26));
        assert_eq!(pressed_key(&report, 6), None);
        assert_eq!(pressed_key(&report[..10], 1), None);
        report[10] = 0;
        assert_eq!(pressed_key(&report, 1), None);
    }
}
