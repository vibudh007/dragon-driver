# Vendor catalog

`SDK_VERSION` preserves the 17-byte, NUL-terminated value returned by `CWitmodSDK_VersionStr`. `SupportedDevice::keyboard` constructs a typed row from the vendor's support table. `Model::new` constructs a model-family row from the model table. The shared vendor fields are keyboard class, type 0 for support rows, and image transparency 0, dimensions 0 × 0, and language count 31 for model rows. There is no one-to-one mapping from a USB ID to a model family in those tables.

The source DLL offsets are `0x3f55c` for the version, `0x29ff0` for `supportHidList`, and `0x2a420` for `hidInfoList`. See the SDK discovery notes.
