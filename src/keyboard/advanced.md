# Advanced key packets

`encode` converts a vendor SDK XML action into one 50-byte onboard slot payload. It uses the same kind numbers as the EXE at `00489940`. It rejects field overflow and invalid storage slots before building a packet.

DKS uses four threshold bytes followed by four eight-byte records. Each record holds one action and its A through D stage flags. The SDK XML reader groups flags by stage, so `encode` transposes them when writing. Mod-Tap uses two actions followed by the time in 10 ms units, with the same integer division as the vendor. Toggle uses one action. The XML Toggle status field stays in the host document. The EXE storage encoder does not read it.

`action_key` packs a direct HID code or delegates a comma-separated modifier combination to the shared binding encoder. An empty field means no key. `byte` bounds stroke and stage values, and `invalid` gives invalid record data a consistent error.

EXE slot builder · [SDK XML action types](../witmod_sdk/profile.md)
