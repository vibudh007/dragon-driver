use crate::witmod_sdk::hid::SDK_REPORT_LEN;

pub type Packet = [u8; SDK_REPORT_LEN];

pub(crate) fn packet(command: u8, header: [u8; 4]) -> Packet {
    let mut report = [0; SDK_REPORT_LEN];
    report[..6].copy_from_slice(&[1, command, header[0], header[1], header[2], header[3]]);
    report
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Reply {
    pub command: u8,
    pub selector: Option<(usize, u8)>,
    pub min_len: usize,
}

impl Reply {
    pub fn matches(self, input: &[u8], report_id: u8) -> bool {
        input.len() >= self.min_len
            && input.get(..2) == Some(&[report_id, self.command])
            && self
                .selector
                .is_none_or(|(offset, value)| input.get(offset) == Some(&value))
    }
}
