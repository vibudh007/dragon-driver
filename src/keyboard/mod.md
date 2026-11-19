# Native keyboard configuration layer

This module ports the EXE's application-level reports and settings on top of the native [Witmod SDK](../witmod_sdk/mod.md). GTK can call the same typed API as the [terminal app](../bin/dragonctl.md).

| Component | Scope |
| --- | --- |
| [Models](models.md) | Match identity and load a vendor key matrix and model capabilities. |
| [Session](session.md) | Own the identified Linux handle. Serialize settings queries and match bounded replies. |
| [Hall-effect settings](hall.md) | Thresholds, Rapid Trigger masks, tuning queries, and calibration message formats. |
| [Performance](performance.md) | Sleep timer, lock, reset, and gamepad message formats. |
| [Lighting](lighting.md) and [audio](audio.md) | Preset settings and frame packets, plus windowed spectrum bands for audio capture integration. |
| [Bindings](bindings.md), [transfer](transfer.md), and [advanced keys](advanced.md) | Compile whole onboard profile plans and encode DKS, Mod-Tap, and Toggle records. |
| [Report](report.md) | Common EXE packet head and response matching. |

Rewrite coverage and remaining evidence
