# Legion Monitor Control Center

Monitor control center for Windows that **tells identical monitors apart**.

The original Lenovo Display Control Center addresses monitors by model name / list position, so two or
more monitors of the same model get mixed up (same EDID, serial and ID). This rebuild identifies every
physical monitor by its Windows display number (`\\.\DISPLAYn`) and its PnP device-instance path, and
sends every DDC/CI command, resolution and rotation change to that exact monitor.

Features:
- Brightness, contrast, color preset, RGB gain and other DDC/CI (VCP) controls per monitor
- Resolution and rotation per monitor
- Per-unit EDID / serial read from each monitor's own registry instance
- "Identificar" shows the Windows display number on each screen
- "Seguir alterações" mirrors changes to monitors you choose explicitly

See [app/MECHANICS.md](app/MECHANICS.md) for how it works.

## Build

Requires Rust (stable) on Windows.

```bash
cd app
cargo build --release
```

Binary: `app/target/release/legion-monitor-control.exe`.

## License

MIT — see [app/LICENSE](app/LICENSE).
