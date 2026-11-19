# System access broker unit

The system service invokes the root-owned daemon with a numeric authorized UID. The process inherits its listener and opens only SDK-validated configuration HID interfaces. Filesystem protection gives it a read-only host view and hides home directories. Its only retained capability is DAC override for opening an authorized device node. Palette storage and audio run in the separate desktop service.
