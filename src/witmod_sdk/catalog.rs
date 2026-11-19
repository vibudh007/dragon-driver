pub const SDK_VERSION: &[u8; 17] = b"V_1.0.1.20240301\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct UsbId {
    pub vendor: u16,
    pub product: u16,
}

#[derive(Clone, Copy, Debug)]
pub struct SupportedDevice {
    pub usb_id: UsbId,
    pub added: &'static str,
    pub controller: &'static str,
}

impl SupportedDevice {
    const fn keyboard(
        vendor: u16,
        product: u16,
        added: &'static str,
        controller: &'static str,
    ) -> Self {
        Self {
            usb_id: UsbId { vendor, product },
            added,
            controller,
        }
    }
}

pub const SUPPORTED_DEVICES: &[SupportedDevice] = &[
    SupportedDevice::keyboard(0x4a4c, 0xe987, "2022/10/24 PM", "JL Keyboard"),
    SupportedDevice::keyboard(0x4a4c, 0xe988, "2022/10/24 PM", "JL Dongle"),
    SupportedDevice::keyboard(0x0416, 0x7372, "2023/07/21 PM", "M484"),
    SupportedDevice::keyboard(0x1fc9, 0x0091, "2023/11/03 PM", "S5516"),
    SupportedDevice::keyboard(0x0416, 0x7472, "2024/06/19 PM", "M484 GamePad Disable"),
    SupportedDevice::keyboard(0x0416, 0x7473, "2024/06/19 PM", "M484 GamePad Xbox360"),
    SupportedDevice::keyboard(0x0416, 0x7474, "2024/06/19 PM", "M484 GamePad Classical"),
    SupportedDevice::keyboard(0x0416, 0x7402, "2024/07/05 PM", "M484"),
];

#[derive(Clone, Copy, Debug)]
pub struct Model {
    pub hid_name: &'static str,
    pub added: &'static str,
    pub controller: &'static str,
    pub key_count: u8,
    pub product_name: &'static str,
    pub fallback_name: &'static str,
}

impl Model {
    const fn new(
        hid_name: &'static str,
        added: &'static str,
        controller: &'static str,
        key_count: u8,
        product_name: &'static str,
        fallback_name: &'static str,
    ) -> Self {
        Self {
            hid_name,
            added,
            controller,
            key_count,
            product_name,
            fallback_name,
        }
    }
}

pub const MODELS: &[Model] = &[
    Model::new(
        "7153_AUSHERGB",
        "2023/11/09 PM",
        "M484 XXX",
        61,
        "M61 MECHANICAL",
        "KEYBOARD",
    ),
    Model::new(
        "7186_AUSHERGB",
        "2024/01/26 AM",
        "M484 XXX",
        68,
        "M68 Mechanical",
        "Keyboard",
    ),
    Model::new(
        "7153_AUSXHERGB",
        "2023/01/26 PM",
        "M484 XXX",
        61,
        "M61 MECHANICAL",
        "KEYBOARD",
    ),
    Model::new(
        "7153_AUSXYXNHJHERGB",
        "2024/05/21 PM",
        "M484 XXX",
        61,
        "M61 MECHANICAL",
        "KEYBOARD",
    ),
    Model::new(
        "7187_AUSXHERGB",
        "2023/06/06 PM",
        "GDXXX XXX",
        98,
        "",
        "M94",
    ),
    Model::new(
        "7186_BUSXYXNHJHERGB",
        "2024/06/11 AM",
        "M484 XXX",
        68,
        "M68 Mechanical",
        "Keyboard",
    ),
    Model::new(
        "7153_AUSXYXNCYHERGB",
        "2024/06/19 PM",
        "M484 XXX",
        61,
        "M61 MECHANICAL",
        "KEYBOARD",
    ),
    Model::new(
        "7272USXYXNCYHERGB",
        "2024/08/01 PM",
        "M484 XXX",
        82,
        "M82 MECHANICAL",
        "KEYBOARD",
    ),
    Model::new(
        "7272USXYXNHJHERGB",
        "2024/08/02 PM",
        "M484 XXX",
        82,
        "M82 MECHANICAL",
        "KEYBOARD",
    ),
];
