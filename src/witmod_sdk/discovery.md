# Linux discovery

`discover_interfaces` reads sysfs once and returns two sorted lists. The candidate list reflects the DLL's supported USB IDs and interface selection. `discover` returns the subset whose report layout can be opened by this Linux SDK. Both paths tolerate a device disappearing during the scan.

`inspect_candidate` matches the HID USB bus and the supported-device catalog. `configuration_interface_number` selects interface 3 for `0416:7473` and `0416:7474` and interface 2 for other listed controllers. The HID path is constructed from a checked node name. `HidCandidate::configuration` checks the descriptor and reads the device number used to validate the opened character device. `inspect` reruns those checks for an existing candidate. `hid_id` and `device_number` parse the kernel values without truncation.

`ConfigInterface::node` and `usb_id` expose the transport identity. `usb_device_path` returns the physical USB device path in sysfs, independent of hidraw numbering. `usb_metadata` reads optional manufacturer and product values and revalidates the interface. `read_usb_metadata` preserves text and strips trailing kernel newlines. `device_number` and `still_present` support revalidation around open. `layout` supplies the matching 64-byte input and output layout and its checked report ID.

The [descriptor parser](descriptor.md) checks the vendor collection and bidirectional report widths, accepting report ID 1 or 6. `HidCandidate::configuration` combines that check with the supported USB catalog and interface rules. `ConfigInterface` retains the actual USB identity, device number, and report ID for reopening and framing.

`usb_device_path` returns the verified configuration interface's USB parent for finding sibling input endpoints. It is used for read-only reactive lighting.

`ConfigInterface::pin_access_node` opens an `O_PATH` descriptor for desktop authorization. It rejects symlinks and non-character nodes, checks the device number against discovery, and revalidates the USB interface before handing the file to the caller. This descriptor does not open the HID transport.
