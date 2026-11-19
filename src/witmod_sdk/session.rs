use std::io;
use std::sync::Mutex;

use super::catalog::{MODELS, Model, SDK_VERSION, SUPPORTED_DEVICES, SupportedDevice};
use super::discovery::{ConfigInterface, HidCandidate, UsbMetadata, discover_interfaces};
use super::hidraw::HidrawDevice;
use super::identity::{Identity, identify};

pub struct SdkSession {
    candidates: Vec<HidCandidate>,
    devices: Vec<ConfigInterface>,
    identity_exchange: Mutex<()>,
    #[cfg(feature = "daemon")]
    access: Access,
}

#[cfg(feature = "daemon")]
#[derive(Clone, Copy)]
pub(crate) enum Access {
    Local,
    Broker,
    SharedBroker,
    SystemBroker,
    Cli,
}

#[derive(Debug)]
pub struct DeviceInfo<'device> {
    pub interface: &'device ConfigInterface,
    pub usb: UsbMetadata,
    pub identity: Identity,
}

impl SdkSession {
    pub fn initialize() -> io::Result<Self> {
        let (candidates, devices) = discover_interfaces()?;
        Ok(Self {
            candidates,
            devices,
            identity_exchange: Mutex::new(()),
            #[cfg(feature = "daemon")]
            access: Access::Local,
        })
    }

    #[cfg(feature = "daemon")]
    pub fn initialize_desktop() -> io::Result<Self> {
        Ok(Self {
            access: Access::Cli,
            ..Self::initialize()?
        })
    }

    #[cfg(feature = "daemon")]
    pub fn initialize_broker() -> io::Result<Self> {
        Ok(Self {
            access: Access::Broker,
            ..Self::initialize()?
        })
    }

    #[cfg(feature = "daemon")]
    pub fn initialize_shared_broker() -> io::Result<Self> {
        Self::initialize_with_access(Access::SharedBroker)
    }

    #[cfg(feature = "daemon")]
    pub fn initialize_system_broker() -> io::Result<Self> {
        Self::initialize_with_access(Access::SystemBroker)
    }

    #[cfg(feature = "daemon")]
    pub(crate) fn access(&self) -> Access {
        self.access
    }

    #[cfg(feature = "daemon")]
    pub(crate) fn initialize_with_access(access: Access) -> io::Result<Self> {
        Ok(Self {
            access,
            ..Self::initialize()?
        })
    }

    pub fn version(&self) -> &'static [u8; 17] {
        SDK_VERSION
    }

    pub fn supported_devices(&self) -> &'static [SupportedDevice] {
        SUPPORTED_DEVICES
    }

    pub fn models(&self) -> &'static [Model] {
        MODELS
    }

    pub fn refresh(&mut self) -> io::Result<()> {
        (self.candidates, self.devices) = discover_interfaces()?;
        Ok(())
    }

    pub fn candidates(&self) -> &[HidCandidate] {
        &self.candidates
    }

    pub fn devices(&self) -> &[ConfigInterface] {
        &self.devices
    }

    pub fn open(&self, device: &ConfigInterface) -> io::Result<HidrawDevice> {
        self.open_interface(device, false)
    }

    fn open_interface(
        &self,
        device: &ConfigInterface,
        nonblocking: bool,
    ) -> io::Result<HidrawDevice> {
        let changed = || io::Error::new(io::ErrorKind::NotFound, "refresh the keyboard list");
        if !self.devices.contains(device) || !device.still_present()? {
            return Err(changed());
        }
        #[cfg(feature = "daemon")]
        match self.access {
            Access::Cli => {
                let handle = HidrawDevice::open_cli(device.node(), device.layout(), nonblocking)?;
                if !device.still_present()? {
                    return Err(changed());
                }
                return Ok(handle);
            }
            Access::SharedBroker => {
                let (file, lease) = crate::service::open_shared_keyboard(device.node())?
                    .ok_or_else(|| io::Error::from(io::ErrorKind::NotConnected))?;
                let mut handle = HidrawDevice::from_file(
                    file,
                    device.layout(),
                    device.device_number(),
                    nonblocking,
                )?;
                if !device.still_present()? {
                    return Err(changed());
                }
                handle.retain_lease(lease);
                return Ok(handle);
            }
            Access::Broker | Access::SystemBroker => {
                let file = match self.access {
                    Access::SystemBroker => crate::service::open_system_keyboard(device.node()),
                    _ => crate::service::open_keyboard(device.node()),
                }?
                .ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "The Dragon Driver configuration service is unavailable",
                    )
                })?;
                let handle = HidrawDevice::from_file(
                    file,
                    device.layout(),
                    device.device_number(),
                    nonblocking,
                )?;
                if !device.still_present()? {
                    return Err(changed());
                }
                return Ok(handle);
            }
            Access::Local => {}
        }
        let handle = HidrawDevice::open_checked(
            device.node(),
            device.layout(),
            device.device_number(),
            nonblocking,
        )?;
        if !device.still_present()? {
            return Err(changed());
        }
        Ok(handle)
    }

    pub fn query_identity(&self, device: &ConfigInterface) -> io::Result<Identity> {
        self.open_identified(device).map(|(_, identity)| identity)
    }

    pub(crate) fn open_identified(
        &self,
        device: &ConfigInterface,
    ) -> io::Result<(HidrawDevice, Identity)> {
        let _exchange = self
            .identity_exchange
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let mut handle = self.open_interface(device, true)?;
        let identity = identify(&mut handle, device.layout().output_id)?;
        Ok((handle, identity))
    }

    #[cfg(feature = "daemon")]
    pub(crate) fn identify_broker_file(
        &self,
        device: &ConfigInterface,
        file: std::fs::File,
    ) -> io::Result<(HidrawDevice, Identity)> {
        if !self.devices.contains(device) || !device.still_present()? {
            return Err(io::Error::from(io::ErrorKind::NotConnected));
        }
        let mut handle =
            HidrawDevice::from_file(file, device.layout(), device.device_number(), true)?;
        if !device.still_present()? {
            return Err(io::Error::from(io::ErrorKind::NotConnected));
        }
        let identity = identify(&mut handle, device.layout().output_id)?;
        Ok((handle, identity))
    }

    pub fn device_info<'device>(
        &self,
        device: &'device ConfigInterface,
    ) -> io::Result<DeviceInfo<'device>> {
        let identity = self.query_identity(device)?;
        Ok(DeviceInfo {
            interface: device,
            usb: device.usb_metadata()?,
            identity,
        })
    }
}
