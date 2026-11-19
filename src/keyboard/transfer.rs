use super::report::{Packet, packet};

pub enum DirectMapping {
    SixKey([u8; 8]),
    FullKey([u8; 13]),
    Multimedia([u8; 4]),
    Mouse { key: u8, action: u8 },
}

impl DirectMapping {
    pub(crate) fn packet(self) -> Packet {
        match self {
            Self::SixKey(data) => six_key_mapping(data),
            Self::FullKey(data) => full_key_mapping(data),
            Self::Multimedia(data) => multimedia_mapping(data),
            Self::Mouse { key, action } => mouse_mapping(key, action),
        }
    }
}

pub fn onboard_chunk(bank: u8, index: u8, payload: &[u8]) -> Option<Packet> {
    if payload.is_empty() || payload.len() > 58 {
        return None;
    }
    let mut report = packet(0x17, [bank, 0, index, payload.len() as u8]);
    report[6..6 + payload.len()].copy_from_slice(payload);
    Some(report)
}

pub fn six_key_mapping(data: [u8; 8]) -> Packet {
    let mut report = packet(3, [0, 0, 0, 9]);
    report[6..14].copy_from_slice(&data);
    report
}

pub fn full_key_mapping(data: [u8; 13]) -> Packet {
    let mut report = packet(4, [0, 0, 0, 21]);
    report[6..19].copy_from_slice(&data);
    report
}

pub fn multimedia_mapping(data: [u8; 4]) -> Packet {
    let mut report = packet(5, [0, 0, 0, 4]);
    report[6..10].copy_from_slice(&data);
    report
}

pub fn mouse_mapping(key: u8, action: u8) -> Packet {
    let mut report = packet(0x12, [0, 0, 0, 6]);
    report[6] = key;
    report[11] = action;
    report
}

pub fn key_chunks(bank: u8, records: &[[u8; 4]]) -> Option<Vec<Packet>> {
    if records.is_empty() || records.len() > 132 {
        return None;
    }
    Some(
        records
            .chunks(14)
            .enumerate()
            .map(|(index, records)| {
                let mut report = packet(0x17, [bank, 0, index as u8, (records.len() * 4) as u8]);
                for (record, output) in records.iter().zip(report[6..].chunks_exact_mut(4)) {
                    output.copy_from_slice(record);
                }
                report
            })
            .collect(),
    )
}

pub fn macro_slot(slot: u8, mode: u8, repeats: u8, events: &[u8]) -> Option<[Packet; 5]> {
    if slot >= 10 || mode == 0 || events.len() > 248 || !events.len().is_multiple_of(4) {
        return None;
    }
    let mut storage = [0; 256];
    storage[1] = mode - 1;
    storage[3] = events.len() as u8;
    storage[5] = repeats;
    storage[8..8 + events.len()].copy_from_slice(events);
    Some(std::array::from_fn(|index| {
        let start = index * 58;
        let end = (start + 58).min(storage.len());
        let mut report = packet(0x18, [slot, 0, index as u8, (end - start) as u8]);
        report[6..6 + end - start].copy_from_slice(&storage[start..end]);
        report
    }))
}

pub fn clear_macro_slot(slot: u8) -> Option<[Packet; 5]> {
    macro_slot(slot, 1, 0, &[])
}

pub fn advanced_slot(slot: u8, format: u8, data: [u8; 50]) -> Option<Packet> {
    if slot >= 40 {
        return None;
    }
    let mut report = packet(0x2a, [slot, 0, format, 50]);
    report[6..56].copy_from_slice(&data);
    Some(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboard_and_remap_payloads_have_vendor_bounds() {
        assert!(onboard_chunk(1, 1, &[0; 59]).is_none());
        let report = onboard_chunk(2, 4, &[13; 58]).unwrap();
        assert_eq!(report[..7], [1, 0x17, 2, 0, 4, 58, 13]);
        assert_eq!(report[63], 13);
        assert_eq!(&six_key_mapping([4; 8])[13..15], &[4, 0]);
        assert_eq!(&full_key_mapping([4; 13])[18..20], &[4, 0]);
        assert_eq!(
            &mouse_mapping(4, 2)[..12],
            &[1, 0x12, 0, 0, 0, 6, 4, 0, 0, 0, 0, 2]
        );
    }

    #[test]
    fn macro_reports_cover_exactly_one_slot() {
        let events = [21; 248];
        let packets = macro_slot(3, 2, 4, &events).unwrap();
        assert_eq!(packets.each_ref().map(|p| p[5]), [58, 58, 58, 58, 24]);
        assert_eq!(&packets[0][6..14], &[0, 1, 0, 248, 0, 4, 0, 0]);
        assert_eq!(&packets[4][6..30], &[21; 24]);
        assert_eq!(
            key_chunks(1, &[[0; 4]; 132]).unwrap().last().unwrap()[5],
            24
        );
    }
}
