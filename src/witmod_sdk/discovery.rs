use std::fs;
use std::io;
use std::num::NonZeroU16;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use super::catalog::{SUPPORTED_DEVICES, UsbId};
use super::descriptor::configuration_report_id;
use super::hid::ReportId;
use super::hidraw::ReportLayout;

pub(super) const HIDRAW_CLASS: &str = "/sys/class/hidraw";
#[cfg(test)]
const M484_USB_ID: UsbId = UsbId {
    vendor: 0x0416,
    product: 0x7372,
};
const USB_BUS: u16 = 3;
const CONFIG_INTERFACE: u8 = 2;
// Vendor usage page ff1b, application usage 91, report ID 1, 63 data bytes each way.
#[cfg(test)]
const CONFIG_DESCRIPTOR: [u8; 32] = [
    0x06, 0x1b, 0xff, 0x09, 0x91, 0xa1, 0x01, 0x85, 0x01, 0x19, 0x00, 0x2a, 0xff, 0x00, 0x15, 0x00,
    0x26, 0xff, 0x00, 0x75, 0x08, 0x95, 0x3f, 0x81, 0x02, 0x19, 0x00, 0x29, 0xff, 0x91, 0x02, 0xc0,
];

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigInterface {
    node: PathBuf,
    link: PathBuf,
    device: PathBuf,
    number: libc::dev_t,
    usb_id: UsbId,
    report_id: ReportId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HidCandidate {
    pub node: PathBuf,
    pub usb_id: UsbId,
    pub interface_number: u8,
    link: PathBuf,
    device: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UsbMetadata {
    pub manufacturer: Option<String>,
    pub product: Option<String>,
}

impl ConfigInterface {
    /// Return the physical USB device path in sysfs, independent of the hidraw node number.
    pub(crate) fn usb_device_path(&self) -> Option<&Path> {
        self.device.parent()?.parent()
    }

    pub fn node(&self) -> &Path {
        &self.node
    }

    pub fn usb_id(&self) -> UsbId {
        self.usb_id
    }

    /// Pin the validated device node for an external permission change.
    /// The O_PATH descriptor exposes metadata without opening a HID session.
    pub fn pin_access_node(&self) -> io::Result<fs::File> {
        let file = fs::File::options()
            .read(true)
            .custom_flags(libc::O_PATH | libc::O_NOFOLLOW)
            .open(&self.node)?;
        let metadata = file.metadata()?;
        if metadata.file_type().is_char_device()
            && metadata.rdev() == self.number
            && self.still_present()?
        {
            Ok(file)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "the configuration interface changed",
            ))
        }
    }

    pub fn usb_metadata(&self) -> io::Result<UsbMetadata> {
        let usb_device =
            self.device.parent().and_then(Path::parent).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "missing USB device path")
            })?;
        let metadata = read_usb_metadata(usb_device)?;
        if self.still_present()? {
            Ok(metadata)
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "the USB interface disappeared",
            ))
        }
    }

    pub(super) fn device_number(&self) -> libc::dev_t {
        self.number
    }

    pub(crate) fn still_present(&self) -> io::Result<bool> {
        match inspect(&self.link) {
            Ok(current) => Ok(current.as_ref() == Some(self)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error),
        }
    }

    pub(super) fn layout(&self) -> ReportLayout {
        ReportLayout {
            input_len: NonZeroU16::new(64).unwrap(),
            output_len: NonZeroU16::new(64).unwrap(),
            output_id: self.report_id,
        }
    }
}

fn read_usb_metadata(usb_device: &Path) -> io::Result<UsbMetadata> {
    let read = |name| match fs::read_to_string(usb_device.join(name)) {
        Ok(text) if text.trim_end_matches('\n').is_empty() => Ok(None),
        Ok(mut text) => {
            text.truncate(text.trim_end_matches('\n').len());
            Ok(Some(text))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    };
    Ok(UsbMetadata {
        manufacturer: read("manufacturer")?,
        product: read("product")?,
    })
}

pub fn discover() -> io::Result<Vec<ConfigInterface>> {
    Ok(discover_interfaces()?.1)
}

pub(super) fn discover_interfaces() -> io::Result<(Vec<HidCandidate>, Vec<ConfigInterface>)> {
    let entries = match fs::read_dir(HIDRAW_CLASS) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok((Vec::new(), Vec::new()));
        }
        Err(error) => return Err(error),
    };
    let mut candidates = Vec::new();
    let mut devices = Vec::new();
    for entry in entries {
        match inspect_candidate(&entry?.path()) {
            Ok(Some(candidate)) => {
                match candidate.configuration() {
                    Ok(Some(device)) => devices.push(device),
                    Ok(None) => {}
                    Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(error),
                }
                candidates.push(candidate);
            }
            Ok(None) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    candidates.sort_unstable_by(|a, b| a.node.cmp(&b.node));
    devices.sort_unstable_by(|a, b| a.node.cmp(&b.node));
    Ok((candidates, devices))
}

fn inspect(link: &Path) -> io::Result<Option<ConfigInterface>> {
    match inspect_candidate(link)? {
        Some(candidate) => candidate.configuration(),
        None => Ok(None),
    }
}

fn inspect_candidate(link: &Path) -> io::Result<Option<HidCandidate>> {
    let Some(name) = link.file_name().and_then(|name| name.to_str()) else {
        return Ok(None);
    };
    if !name
        .strip_prefix("hidraw")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
    {
        return Ok(None);
    }
    let device = fs::canonicalize(link.join("device"))?;
    let event = fs::read_to_string(device.join("uevent"))?;
    let Some((USB_BUS, usb_id)) = hid_id(&event) else {
        return Ok(None);
    };
    if !SUPPORTED_DEVICES
        .iter()
        .any(|device| device.usb_id == usb_id)
    {
        return Ok(None);
    }
    let Some(interface) = device.parent() else {
        return Ok(None);
    };
    let interface_number = fs::read_to_string(interface.join("bInterfaceNumber"))?;
    let Ok(interface_number) = u8::from_str_radix(interface_number.trim(), 16) else {
        return Ok(None);
    };
    if interface_number != configuration_interface_number(usb_id) {
        return Ok(None);
    }
    Ok(Some(HidCandidate {
        node: Path::new("/dev").join(name),
        usb_id,
        interface_number,
        link: link.to_path_buf(),
        device,
    }))
}

impl HidCandidate {
    fn configuration(&self) -> io::Result<Option<ConfigInterface>> {
        let descriptor = fs::read(self.device.join("report_descriptor"))?;
        let Some(report_id) = configuration_report_id(&descriptor) else {
            return Ok(None);
        };
        let dev = fs::read_to_string(self.link.join("dev"))?;
        let number = device_number(&dev).ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidData, "invalid hidraw device number")
        })?;
        Ok(Some(ConfigInterface {
            node: self.node.clone(),
            link: self.link.clone(),
            device: self.device.clone(),
            number,
            usb_id: self.usb_id,
            report_id,
        }))
    }
}

fn configuration_interface_number(usb_id: UsbId) -> u8 {
    match (usb_id.vendor, usb_id.product) {
        (0x0416, 0x7473 | 0x7474) => 3,
        _ => CONFIG_INTERFACE,
    }
}

fn hid_id(event: &str) -> Option<(u16, UsbId)> {
    let value = event
        .lines()
        .find_map(|line| line.strip_prefix("HID_ID="))?;
    let (bus, pair) = value.split_once(':')?;
    let (vendor, product) = pair.split_once(':')?;
    Some((
        u16::from_str_radix(bus, 16).ok()?,
        UsbId {
            vendor: u16::from_str_radix(vendor, 16).ok()?,
            product: u16::from_str_radix(product, 16).ok()?,
        },
    ))
}

pub(super) fn device_number(raw: &str) -> Option<libc::dev_t> {
    let (major, minor) = raw.trim().split_once(':')?;
    Some(libc::makedev(major.parse().ok()?, minor.parse().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_kernel_ids_without_truncating_malformed_values() {
        assert_eq!(
            hid_id("DRIVER=hid-generic\nHID_ID=0003:00000416:00007372\n"),
            Some((USB_BUS, M484_USB_ID))
        );
        assert_eq!(hid_id("HID_ID=0003:00010416:00007372"), None);
        assert_eq!(device_number("241:4\n"), Some(libc::makedev(241, 4)));
    }

    #[test]
    fn uses_the_vendor_interface_rule_for_gamepad_variants() {
        for product in [0x7372, 0x7472, 0x7473, 0x7474] {
            let id = UsbId {
                vendor: 0x0416,
                product,
            };
            assert_eq!(
                configuration_interface_number(id),
                if product >= 0x7473 { 3 } else { 2 }
            );
        }
    }

    #[test]
    fn same_collection_can_use_either_vendor_report_id() {
        let mut descriptor = CONFIG_DESCRIPTOR;
        assert_eq!(
            configuration_report_id(&descriptor),
            Some(ReportId::Default)
        );
        descriptor[8] = 6;
        assert_eq!(
            configuration_report_id(&descriptor),
            Some(ReportId::Collection06)
        );
        descriptor[8] = 2;
        assert_eq!(configuration_report_id(&descriptor), None);
        descriptor[8] = 6;
        descriptor[24] = 62;
        assert_eq!(configuration_report_id(&descriptor), None);
        assert_eq!(configuration_report_id(&[0xc0]), None);
    }
}
