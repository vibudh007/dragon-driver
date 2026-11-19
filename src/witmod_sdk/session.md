# SDK session

- `SdkSession::initialize()` creates an owned session and reads the current sysfs device list. It performs no device I/O.
- `version()`, `supported_devices()`, and `models()` expose the original version and static catalog. `candidates()` borrows the catalog-matched interfaces and `devices()` borrows the verified transports.
- `refresh()` rescans and replaces the two lists on success.
- `open(device)` requires a candidate from the current list, revalidates it before and after opening the node, and compares the character-device number on the opened file. The caller owns the returned handle. A device change or permission failure is reported as an I/O error.
- `query_identity(device)` serializes identity requests within the session and uses a fresh checked nonblocking handle. `open_interface` shares the candidate and device validation with the normal blocking open. This is synchronous and belongs on the future device worker.

- `device_info(device)` returns the checked interface, optional USB names, and decoded firmware identity together. Its result holds a borrow to the original discovery record. See the identity result mapping.

This replaces the Windows global initialization flag with Rust ownership. The Windows SDK's timestamp gate and DLL-loading state have no role in Linux device access. The user verified the identity exchange on the attached K617M. `Identity::model()` resolves its exact layout code to the 61-key catalog entry. Confirmed device identity

`open_identified` returns the nonblocking handle and identity from the same open operation. The keyboard settings layer keeps this handle for subsequent requests.

`identify_broker_file` checks the supplied descriptor against the verified configuration interface and confirms it is still present before issuing the identity exchange. It is used before the daemon passes a newly restored keyboard lease to a client.
