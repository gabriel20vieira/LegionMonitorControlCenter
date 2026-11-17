# Mechanics sheet — Monitor Control Center (rebuild of Lenovo Display Control Center 3.0.32161)

## Where the original keeps its logic
- UI: Vue web app in `Assets/html/normal` (app.fabd9a3ca68fcfa35fcd.js). It talks to a background
  service through `api.sendRequest([{function, par}])`.
- Monitor control: `MONITORCONTROL.dll` inside `servicefiles/LenovoDCCServiceInstaller.msi`
  (exports `_MC_InitMonitors`, `_MC_GetMonitorID`, `_MC_SetVCPFeature`, … ; uses dxva2 DDC/CI).

## How the original identifies monitors (the bug the person reported)
- UI: list from `InitArchitecture` → `monitorList[]` with `index`, `name`, `active`; switching calls
  `ChangeCurDisplayMonitorIndex {index}` (NHnr.js, `currentMonitorChange`). Options keyed `name + index`.
- DLL: `_MC_GetMonitorCount@0` (0x10003950) builds a vector of 0x1AF0-byte records with
  `EnumDisplayMonitors(…, 0x10002d80, …)`; every accessor (`_MC_GetMonitorID` 0x10003ce0,
  `_MC_GetMonitorSN` 0x10003bc0, `_MC_SetVCPFeature` 0x10003e00) is just *position in that vector*.
- Callback 0x10002d80: when the adapter's display-device count differs from the physical-monitor count
  it copies the **first active** monitor device into every record; the monitor ID string is cut from the
  device id (after the last `-`, `}`→`-`, `\`→`0`).
- EDID: 0x10001820 opens `HKLM\SYSTEM\CurrentControlSet\Enum\DISPLAY`, reads key index 0
  (`RegEnumKeyW(…,0,…)`) and the model sub-key — **not the monitor's own instance** — so two identical
  models get the same EDID, serial and model id. Result: the service cannot tell twins apart.

## How the rebuild identifies monitors
- One entry per physical monitor: `EnumDisplayMonitors` → `GetMonitorInfoW` (`\\.\DISPLAYn`) →
  `EnumDisplayDevicesW(…, EDD_GET_DEVICE_INTERFACE_NAME)` (active devices, in order) paired with
  `GetPhysicalMonitorsFromHMONITOR` (same order).
- Unique key = monitor device-interface path (contains PnP instance id, e.g. `…&UID4352`).
- Shown number = Windows display number `n` from `\\.\DISPLAYn`.
- EDID read from that instance's own key `Enum\DISPLAY\<hw>\<instance>\Device Parameters\EDID`
  → real per-unit serial.
- Every DDC command and every resolution/rotation change is addressed by key / `\\.\DISPLAYn`,
  never by model name. "Seguir alterações" lets the person mirror changes to chosen monitors explicitly.
- "Identificar" draws the number on each screen (2500 ms, estimated — the original has no equivalent).

## DDC/CI feature codes (from VCPCodeDef.json v1.7 in the service MSI)
| code | feature | values |
|---|---|---|
| 0x10 | Brightness | continuous 0–0x64 |
| 0x12 | Contrast | continuous 0–0x64 |
| 0x14 | Color preset | 01 sRGB, 02 native, 05 6500K, 06 7500K, 08 9300K, 0B–0D custom, 0E BT.709, 0F DCI-P3, 10 BT.2020, 11 Adobe RGB, 12 LBL, 13 DICOM |
| 0x16/0x18/0x1A | R/G/B gain | continuous 0–0x64 |
| 0x60 | Input source | 0F DP1, 10 DP2, 11–14 HDMI1–4, 31/32 USB-C1/2, 03/04 DVI, FE auto … |
| 0x86 | Display scaling | 01 none, 02 full, 05 original AR, 0B 4:3, 0F 16:9, 10 16:10 |
| 0xDC | Scenario mode | 00 native, 01 image creation, 03 cinema, 05 video, 06 LBL, 07 web, 08 gaming, 09 pro |
| 0xE0 | Over Drive | 00 off, 01 normal, 02 extreme, 03–07 level 1–5 |
| 0xEA | Dynamic contrast | 00 off, 01 on |
| 0xEF | HDR | 00 off, 01 auto, 02 HDR1000, 03 HLG, 04 DV, 05 HDR, 06 HDR400, 07–09 game/movie/photo, 0C HDR10 |
| 0xF9 | Game mode | 00 off, 01 FPS1, 02 FPS2, 03 RCG, 04 RTS, 05 G1, 06 G2, 07 MOBA, 08 ACT, 09 RPG, 0A standard |
| 0xD6 | Power mode | 01 on, 04 standby, 05 off |
| 0x04 | Restore factory | write 1 |

Options are filtered by the monitor's own MCCS capabilities string when it lists values.

## Resolution / refresh / rotation
- `MONITORCONTROL.dll` imports `EnumDisplaySettingsW`, `ChangeDisplaySettingsExW`
  (`_MC_SetMonitorResolution@12` 0x10004ad0, `_MC_SetMonitorFreq@8` 0x100052a0,
  `_MC_SetMonitorOrientation@8` 0x10004d60). Rebuild does the same per `\\.\DISPLAYn`
  with `CDS_UPDATEREGISTRY`.

## Look
- Palette from `static/style/artery/common.css`: #1f2a42 background, #2e3b56 panels, #4990ff accent,
  #f1f1f1 text, #8ab1ed secondary text, #fd4c4c warnings.
