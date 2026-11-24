//! Windows display enumeration, DDC/CI (dxva2) and display-mode calls.
//!
//! The key difference from the original: every physical monitor gets a stable
//! identity made of the Windows display number (\\.\DISPLAYn) plus the PnP
//! device-instance id of the monitor (…&UIDnnnn), and its EDID is read from
//! *that* instance's registry key. Two identical models therefore never share
//! an identity, a serial number or a DDC handle.
#![allow(non_snake_case, clippy::upper_case_acronyms, non_camel_case_types)]

use std::ffi::c_void;

pub type HANDLE = isize;
pub type HMONITOR = isize;
pub type HDC = isize;
pub type HWND = isize;
pub type BOOL = i32;
pub type LPARAM = isize;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct RECT {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

#[repr(C)]
struct MONITORINFOEXW {
    cbSize: u32,
    rcMonitor: RECT,
    rcWork: RECT,
    dwFlags: u32,
    szDevice: [u16; 32],
}

#[repr(C)]
struct DISPLAY_DEVICEW {
    cb: u32,
    DeviceName: [u16; 32],
    DeviceString: [u16; 128],
    StateFlags: u32,
    DeviceID: [u16; 128],
    DeviceKey: [u16; 128],
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PHYSICAL_MONITOR {
    h: HANDLE,
    desc: [u16; 128],
}

#[repr(C)]
#[derive(Clone, Copy)]
pub struct DEVMODEW {
    pub dmDeviceName: [u16; 32],
    pub dmSpecVersion: u16,
    pub dmDriverVersion: u16,
    pub dmSize: u16,
    pub dmDriverExtra: u16,
    pub dmFields: u32,
    pub dmPositionX: i32,
    pub dmPositionY: i32,
    pub dmDisplayOrientation: u32,
    pub dmDisplayFixedOutput: u32,
    pub dmColor: i16,
    pub dmDuplex: i16,
    pub dmYResolution: i16,
    pub dmTTOption: i16,
    pub dmCollate: i16,
    pub dmFormName: [u16; 32],
    pub dmLogPixels: u16,
    pub dmBitsPerPel: u32,
    pub dmPelsWidth: u32,
    pub dmPelsHeight: u32,
    pub dmDisplayFlags: u32,
    pub dmDisplayFrequency: u32,
    pub dmICMMethod: u32,
    pub dmICMIntent: u32,
    pub dmMediaType: u32,
    pub dmDitherType: u32,
    pub dmReserved1: u32,
    pub dmReserved2: u32,
    pub dmPanningWidth: u32,
    pub dmPanningHeight: u32,
}

type MonitorEnumProc = unsafe extern "system" fn(HMONITOR, HDC, *mut RECT, LPARAM) -> BOOL;

#[link(name = "user32")]
extern "system" {
    fn EnumDisplayMonitors(hdc: HDC, clip: *const RECT, cb: Option<MonitorEnumProc>, data: LPARAM) -> BOOL;
    fn GetMonitorInfoW(h: HMONITOR, mi: *mut MONITORINFOEXW) -> BOOL;
    fn EnumDisplayDevicesW(dev: *const u16, i: u32, dd: *mut DISPLAY_DEVICEW, flags: u32) -> BOOL;
    fn EnumDisplaySettingsW(dev: *const u16, mode: u32, dm: *mut DEVMODEW) -> BOOL;
    fn ChangeDisplaySettingsExW(dev: *const u16, dm: *const DEVMODEW, hwnd: HWND, flags: u32, lparam: *const c_void) -> i32;
}

#[link(name = "dxva2")]
extern "system" {
    fn GetNumberOfPhysicalMonitorsFromHMONITOR(h: HMONITOR, n: *mut u32) -> BOOL;
    fn GetPhysicalMonitorsFromHMONITOR(h: HMONITOR, n: u32, arr: *mut PHYSICAL_MONITOR) -> BOOL;
    fn DestroyPhysicalMonitor(h: HANDLE) -> BOOL;
    fn GetVCPFeatureAndVCPFeatureReply(h: HANDLE, code: u8, ty: *mut u32, cur: *mut u32, max: *mut u32) -> BOOL;
    fn SetVCPFeature(h: HANDLE, code: u8, val: u32) -> BOOL;
    fn GetCapabilitiesStringLength(h: HANDLE, len: *mut u32) -> BOOL;
    fn CapabilitiesRequestAndCapabilitiesReply(h: HANDLE, s: *mut u8, len: u32) -> BOOL;
}

#[link(name = "advapi32")]
extern "system" {
    fn RegOpenKeyExW(hkey: isize, sub: *const u16, opts: u32, sam: u32, out: *mut isize) -> i32;
    fn RegQueryValueExW(hkey: isize, name: *const u16, res: *mut u32, ty: *mut u32, data: *mut u8, len: *mut u32) -> i32;
    fn RegCloseKey(hkey: isize) -> i32;
}

const HKEY_LOCAL_MACHINE: isize = 0x8000_0002u32 as i32 as isize;
const KEY_READ: u32 = 0x20019;
const EDD_GET_DEVICE_INTERFACE_NAME: u32 = 1;
const DISPLAY_DEVICE_ACTIVE: u32 = 1;
const MONITORINFOF_PRIMARY: u32 = 1;
const ENUM_CURRENT_SETTINGS: u32 = 0xFFFF_FFFF;
const DM_DISPLAYORIENTATION: u32 = 0x0000_0080;
const DM_PELSWIDTH: u32 = 0x0008_0000;
const DM_PELSHEIGHT: u32 = 0x0010_0000;
const DM_DISPLAYFREQUENCY: u32 = 0x0040_0000;
const CDS_UPDATEREGISTRY: u32 = 1;
const CDS_NORESET: u32 = 0x1000_0000;
const DM_POSITION: u32 = 0x0000_0020;

pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn from_wide(w: &[u16]) -> String {
    let end = w.iter().position(|&c| c == 0).unwrap_or(w.len());
    String::from_utf16_lossy(&w[..end])
}

/// Everything we know about one physical monitor.
#[derive(Clone, Debug, Default)]
pub struct MonitorDesc {
    /// Stable unique key: the monitor's device interface path (contains the PnP instance id).
    pub key: String,
    /// GDI device name, e.g. `\\.\DISPLAY2`.
    pub gdi_name: String,
    /// Windows display number (the `n` in DISPLAYn).
    pub win_number: u32,
    /// Index of this physical monitor inside its HMONITOR (0 unless cloned).
    pub sub_index: usize,
    pub name: String,
    pub manufacturer: String,
    pub product: u16,
    pub hardware_id: String,
    pub serial_number: u32,
    pub serial_text: String,
    /// Short unique part of the PnP instance id, e.g. `UID4352`.
    pub instance_short: String,
    /// Full PnP instance path, e.g. `DISPLAY\LEN66F2\5&2b6e&0&UID4352`.
    pub instance_path: String,
    pub year: u16,
    pub rect: RECT,
    pub primary: bool,
    pub ddc: bool,
    /// Connected but not part of the desktop (turned off here or in Windows settings).
    pub off: bool,
}

impl MonitorDesc {
    /// Text shown wherever the monitor is listed: number + name.
    pub fn title(&self) -> String {
        format!("{} · {}", self.win_number, self.name)
    }
    pub fn serial_display(&self) -> String {
        if !self.serial_text.is_empty() {
            self.serial_text.clone()
        } else if self.serial_number != 0 {
            format!("{}", self.serial_number)
        } else {
            "—".to_string()
        }
    }
    pub fn width(&self) -> i32 {
        self.rect.right - self.rect.left
    }
    pub fn height(&self) -> i32 {
        self.rect.bottom - self.rect.top
    }
}

unsafe extern "system" fn collect_monitor(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
    let v = &mut *(data as *mut Vec<HMONITOR>);
    v.push(h);
    1
}

fn display_number(gdi: &str) -> u32 {
    let digits: String = gdi.chars().rev().take_while(|c| c.is_ascii_digit()).collect();
    digits.chars().rev().collect::<String>().parse().unwrap_or(0)
}

/// `\\?\DISPLAY#LEN66F2#5&2b6e6f2&0&UID4352#{e6f07b5f-...}` → (`DISPLAY\LEN66F2\5&2b6e6f2&0&UID4352`, `LEN66F2`, `UID4352`)
fn parse_interface(iface: &str) -> Option<(String, String, String)> {
    let s = iface.strip_prefix(r"\\?\").unwrap_or(iface);
    let s = match s.find("#{") {
        Some(i) => &s[..i],
        None => s,
    };
    let parts: Vec<&str> = s.split('#').collect();
    if parts.len() < 3 {
        return None;
    }
    let hw = parts[1].to_string();
    let inst = parts[2];
    let short = inst.rsplit('&').next().unwrap_or(inst).to_string();
    Some((parts.join("\\"), hw, short))
}

fn read_edid(instance_path: &str) -> Option<Vec<u8>> {
    unsafe {
        let sub = wide(&format!(r"SYSTEM\CurrentControlSet\Enum\{}\Device Parameters", instance_path));
        let mut key: isize = 0;
        if RegOpenKeyExW(HKEY_LOCAL_MACHINE, sub.as_ptr(), 0, KEY_READ, &mut key) != 0 {
            return None;
        }
        let name = wide("EDID");
        let mut len: u32 = 0;
        let mut out = None;
        if RegQueryValueExW(key, name.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(), &mut len) == 0 && len > 0 {
            let mut buf = vec![0u8; len as usize];
            if RegQueryValueExW(key, name.as_ptr(), std::ptr::null_mut(), std::ptr::null_mut(), buf.as_mut_ptr(), &mut len) == 0 {
                buf.truncate(len as usize);
                out = Some(buf);
            }
        }
        RegCloseKey(key);
        out
    }
}

struct MonitorDevice {
    iface: String,
    description: String,
}

fn monitor_devices(gdi: &str) -> Vec<MonitorDevice> {
    let mut all = Vec::new();
    let mut active = Vec::new();
    let gdi_w = wide(gdi);
    unsafe {
        for i in 0..16u32 {
            let mut dd: DISPLAY_DEVICEW = std::mem::zeroed();
            dd.cb = std::mem::size_of::<DISPLAY_DEVICEW>() as u32;
            if EnumDisplayDevicesW(gdi_w.as_ptr(), i, &mut dd, EDD_GET_DEVICE_INTERFACE_NAME) == 0 {
                break;
            }
            let dev = MonitorDevice {
                iface: from_wide(&dd.DeviceID),
                description: from_wide(&dd.DeviceString),
            };
            if dd.StateFlags & DISPLAY_DEVICE_ACTIVE != 0 {
                active.push(dev);
            } else {
                all.push(dev);
            }
        }
    }
    if active.is_empty() {
        all
    } else {
        active
    }
}

/// Enumerate every physical monitor. Returns descriptions plus the DDC handle
/// (if Windows gave one). The caller owns the handles and must `destroy_handle` them.
pub fn enumerate() -> Vec<(MonitorDesc, Option<HANDLE>)> {
    let mut hmons: Vec<HMONITOR> = Vec::new();
    unsafe {
        EnumDisplayMonitors(0, std::ptr::null(), Some(collect_monitor), &mut hmons as *mut _ as LPARAM);
    }
    let mut out = Vec::new();
    for hmon in hmons {
        let mut mi: MONITORINFOEXW = unsafe { std::mem::zeroed() };
        mi.cbSize = std::mem::size_of::<MONITORINFOEXW>() as u32;
        if unsafe { GetMonitorInfoW(hmon, &mut mi) } == 0 {
            continue;
        }
        let gdi = from_wide(&mi.szDevice);
        let devices = monitor_devices(&gdi);

        let mut count: u32 = 0;
        let mut phys: Vec<PHYSICAL_MONITOR> = Vec::new();
        unsafe {
            if GetNumberOfPhysicalMonitorsFromHMONITOR(hmon, &mut count) != 0 && count > 0 {
                phys = vec![PHYSICAL_MONITOR { h: 0, desc: [0; 128] }; count as usize];
                if GetPhysicalMonitorsFromHMONITOR(hmon, count, phys.as_mut_ptr()) == 0 {
                    phys.clear();
                }
            }
        }
        let n = phys.len().max(devices.len()).max(1);
        for j in 0..n {
            let dev = devices.get(j);
            let ph = phys.get(j);
            let iface = dev.map(|d| d.iface.clone()).unwrap_or_default();
            let mut desc = MonitorDesc {
                gdi_name: gdi.clone(),
                win_number: display_number(&gdi),
                sub_index: j,
                rect: mi.rcMonitor,
                primary: mi.dwFlags & MONITORINFOF_PRIMARY != 0,
                ..Default::default()
            };
            if let Some((path, hw, short)) = parse_interface(&iface) {
                desc.instance_path = path;
                desc.hardware_id = hw;
                desc.instance_short = short;
            }
            desc.key = if iface.is_empty() {
                format!("{}#{}", gdi, j)
            } else {
                iface.to_uppercase()
            };
            if let Some(e) = read_edid(&desc.instance_path).as_deref().and_then(crate::edid::parse) {
                desc.manufacturer = e.manufacturer;
                desc.product = e.product;
                desc.serial_number = e.serial_number;
                desc.serial_text = e.serial_text.unwrap_or_default();
                desc.year = e.year;
                if let Some(nm) = e.name {
                    desc.name = nm;
                }
            }
            if desc.name.is_empty() {
                desc.name = ph
                    .map(|p| from_wide(&p.desc))
                    .filter(|s| !s.is_empty())
                    .or_else(|| dev.map(|d| d.description.clone()))
                    .unwrap_or_else(|| "Monitor".to_string());
            }
            let handle = ph.map(|p| p.h).filter(|h| *h != 0);
            desc.ddc = handle.is_some();
            out.push((desc, handle));
        }
        // Destroy handles we did not hand out (more physical monitors than slots is impossible, kept for safety).
        for p in phys.iter().skip(n) {
            unsafe { DestroyPhysicalMonitor(p.h) };
        }
    }
    let active: std::collections::HashSet<String> = out.iter().map(|(d, _)| d.key.clone()).collect();
    out.extend(detached().into_iter().filter(|d| !active.contains(&d.key)).map(|d| (d, None)));
    out.sort_by(|a, b| (a.0.win_number, a.0.sub_index).cmp(&(b.0.win_number, b.0.sub_index)));
    out
}


pub fn destroy_handle(h: HANDLE) {
    unsafe {
        DestroyPhysicalMonitor(h);
    }
}

/// Read a VCP feature: (current, maximum).
pub fn get_vcp(h: HANDLE, code: u8) -> Option<(u32, u32)> {
    let (mut ty, mut cur, mut max) = (0u32, 0u32, 0u32);
    let ok = unsafe { GetVCPFeatureAndVCPFeatureReply(h, code, &mut ty, &mut cur, &mut max) };
    (ok != 0).then_some((cur, max))
}

pub fn set_vcp(h: HANDLE, code: u8, value: u32) -> bool {
    unsafe { SetVCPFeature(h, code, value) != 0 }
}

pub fn capabilities(h: HANDLE) -> Option<String> {
    unsafe {
        let mut len = 0u32;
        if GetCapabilitiesStringLength(h, &mut len) == 0 || len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        if CapabilitiesRequestAndCapabilitiesReply(h, buf.as_mut_ptr(), len) == 0 {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        Some(String::from_utf8_lossy(&buf[..end]).to_string())
    }
}

// ---------------------------------------------------------------- display modes

#[derive(Clone, Debug, Default)]
pub struct ModeInfo {
    pub width: u32,
    pub height: u32,
    pub freq: u32,
    pub orientation: u32,
    /// (width, height) → refresh rates
    pub modes: Vec<((u32, u32), Vec<u32>)>,
}

fn zero_devmode() -> DEVMODEW {
    let mut dm: DEVMODEW = unsafe { std::mem::zeroed() };
    dm.dmSize = std::mem::size_of::<DEVMODEW>() as u16;
    dm
}

fn current_devmode(gdi: &str) -> Option<DEVMODEW> {
    let g = wide(gdi);
    let mut dm = zero_devmode();
    (unsafe { EnumDisplaySettingsW(g.as_ptr(), ENUM_CURRENT_SETTINGS, &mut dm) } != 0).then_some(dm)
}

pub fn mode_info(gdi: &str) -> ModeInfo {
    let mut info = ModeInfo::default();
    if let Some(dm) = current_devmode(gdi) {
        info.width = dm.dmPelsWidth;
        info.height = dm.dmPelsHeight;
        info.freq = dm.dmDisplayFrequency;
        info.orientation = dm.dmDisplayOrientation;
    }
    let g = wide(gdi);
    let mut map: std::collections::BTreeMap<(u32, u32), std::collections::BTreeSet<u32>> = Default::default();
    let mut i = 0u32;
    loop {
        let mut dm = zero_devmode();
        if unsafe { EnumDisplaySettingsW(g.as_ptr(), i, &mut dm) } == 0 {
            break;
        }
        if dm.dmBitsPerPel >= 24 || dm.dmBitsPerPel == 0 {
            let (mut w, mut h) = (dm.dmPelsWidth, dm.dmPelsHeight);
            // Modes are listed in the panel's native orientation; show them as they'll look now.
            if info.orientation % 2 == 1 && w > h {
                std::mem::swap(&mut w, &mut h);
            }
            map.entry((w, h)).or_default().insert(dm.dmDisplayFrequency);
        }
        i += 1;
        if i > 4096 {
            break;
        }
    }
    let mut modes: Vec<((u32, u32), Vec<u32>)> = map
        .into_iter()
        .map(|(k, v)| (k, v.into_iter().rev().collect()))
        .collect();
    modes.sort_by(|a, b| (b.0 .0 * b.0 .1).cmp(&(a.0 .0 * a.0 .1)).then(b.0 .0.cmp(&a.0 .0)));
    info.modes = modes;
    info
}

fn change_result(code: i32) -> Result<(), String> {
    match code {
        0 => Ok(()),
        1 => Err("É preciso reiniciar o computador para aplicar.".into()),
        -1 => Err("O controlador gráfico recusou o modo.".into()),
        -2 => Err("Modo não suportado por este monitor.".into()),
        -3 => Err("Não foi possível guardar o modo no registo.".into()),
        -4 => Err("Parâmetros inválidos.".into()),
        -5 => Err("Parâmetros inválidos.".into()),
        -6 => Err("Este ecrã está ligado a outro adaptador.".into()),
        x => Err(format!("Falhou (código {x}).")),
    }
}

/// Change resolution / refresh rate of exactly this Windows display (\\.\DISPLAYn).
pub fn apply_mode(gdi: &str, width: u32, height: u32, freq: u32) -> Result<(), String> {
    let mut dm = current_devmode(gdi).ok_or("Não foi possível ler o modo atual.")?;
    dm.dmPelsWidth = width;
    dm.dmPelsHeight = height;
    dm.dmDisplayFrequency = freq;
    dm.dmFields = DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYFREQUENCY;
    let g = wide(gdi);
    change_result(unsafe { ChangeDisplaySettingsExW(g.as_ptr(), &dm, 0, CDS_UPDATEREGISTRY, std::ptr::null()) })
}

/// Rotate exactly this Windows display. orientation: 0=0°, 1=90°, 2=180°, 3=270°.
pub fn apply_orientation(gdi: &str, orientation: u32) -> Result<(), String> {
    let mut dm = current_devmode(gdi).ok_or("Não foi possível ler o modo atual.")?;
    if dm.dmDisplayOrientation % 2 != orientation % 2 {
        std::mem::swap(&mut dm.dmPelsWidth, &mut dm.dmPelsHeight);
    }
    dm.dmDisplayOrientation = orientation;
    dm.dmFields = DM_DISPLAYORIENTATION | DM_PELSWIDTH | DM_PELSHEIGHT;
    let g = wide(gdi);
    change_result(unsafe { ChangeDisplaySettingsExW(g.as_ptr(), &dm, 0, CDS_UPDATEREGISTRY, std::ptr::null()) })
}

// ---------------------------------------------------------------- turning screens on and off
//
// Uses the CCD API (QueryDisplayConfig / SetDisplayConfig). Once a monitor is off, Windows no
// longer ties it to a \\.\DISPLAYn output (every free output lists every off monitor), so the
// old per-output API cannot tell which monitor to light up. CCD paths name the physical
// monitor by its device path, the same unique key the rest of the app uses.

#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct LUID {
    low: u32,
    high: i32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PathSource {
    adapter: LUID,
    id: u32,
    mode_idx: u32,
    status: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PathTarget {
    adapter: LUID,
    id: u32,
    mode_idx: u32,
    technology: u32,
    rotation: u32,
    scaling: u32,
    refresh_num: u32,
    refresh_den: u32,
    scanline: u32,
    available: BOOL,
    status: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct PathInfo {
    source: PathSource,
    target: PathTarget,
    flags: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
struct CcdMode {
    info_type: u32,
    id: u32,
    adapter: LUID,
    data: [u64; 6],
}

#[repr(C)]
struct DevInfoHeader {
    ty: u32,
    size: u32,
    adapter: LUID,
    id: u32,
}

#[repr(C)]
struct TargetName {
    header: DevInfoHeader,
    flags: u32,
    technology: u32,
    edid_manufacturer: u16,
    edid_product: u16,
    connector: u32,
    friendly: [u16; 64],
    path: [u16; 128],
}

#[repr(C)]
struct SourceName {
    header: DevInfoHeader,
    gdi: [u16; 32],
}

#[link(name = "user32")]
extern "system" {
    fn GetDisplayConfigBufferSizes(flags: u32, paths: *mut u32, modes: *mut u32) -> i32;
    fn QueryDisplayConfig(flags: u32, np: *mut u32, paths: *mut PathInfo, nm: *mut u32, modes: *mut CcdMode, topology: *mut u32) -> i32;
    fn SetDisplayConfig(np: u32, paths: *const PathInfo, nm: u32, modes: *const CcdMode, flags: u32) -> i32;
    fn DisplayConfigGetDeviceInfo(header: *mut DevInfoHeader) -> i32;
}

const QDC_ALL_PATHS: u32 = 0x1;
const SDC_APPLY: u32 = 0x80;
const SDC_USE_SUPPLIED_DISPLAY_CONFIG: u32 = 0x20;
const SDC_SAVE_TO_DATABASE: u32 = 0x200;
const SDC_ALLOW_CHANGES: u32 = 0x400;
const PATH_ACTIVE: u32 = 0x1;
const MODE_IDX_INVALID: u32 = 0xFFFF_FFFF;
const ERROR_INSUFFICIENT_BUFFER: i32 = 122;

fn query(flags: u32) -> Result<(Vec<PathInfo>, Vec<CcdMode>), String> {
    for _ in 0..5 {
        let (mut np, mut nm) = (0u32, 0u32);
        let r = unsafe { GetDisplayConfigBufferSizes(flags, &mut np, &mut nm) };
        if r != 0 {
            return Err(format!("GetDisplayConfigBufferSizes falhou ({r})."));
        }
        let mut paths = vec![PathInfo::default(); np as usize];
        let mut modes = vec![CcdMode::default(); nm as usize];
        let r = unsafe { QueryDisplayConfig(flags, &mut np, paths.as_mut_ptr(), &mut nm, modes.as_mut_ptr(), std::ptr::null_mut()) };
        if r == ERROR_INSUFFICIENT_BUFFER {
            continue; // a screen came or went between the two calls
        }
        if r != 0 {
            return Err(format!("QueryDisplayConfig falhou ({r})."));
        }
        paths.truncate(np as usize);
        modes.truncate(nm as usize);
        return Ok((paths, modes));
    }
    Err("A configuração de ecrãs mudou enquanto era lida.".into())
}

/// Device path of the monitor on a target, uppercased like `MonitorDesc::key`, plus its friendly name.
fn target_name(adapter: LUID, id: u32) -> (String, String) {
    let mut t: TargetName = unsafe { std::mem::zeroed() };
    t.header = DevInfoHeader { ty: 2, size: std::mem::size_of::<TargetName>() as u32, adapter, id };
    if unsafe { DisplayConfigGetDeviceInfo(&mut t.header) } != 0 {
        return (String::new(), String::new());
    }
    (from_wide(&t.path).to_uppercase(), from_wide(&t.friendly))
}

/// `\\.\DISPLAYn` of a source.
fn source_gdi(adapter: LUID, id: u32) -> String {
    let mut s: SourceName = unsafe { std::mem::zeroed() };
    s.header = DevInfoHeader { ty: 1, size: std::mem::size_of::<SourceName>() as u32, adapter, id };
    if unsafe { DisplayConfigGetDeviceInfo(&mut s.header) } != 0 {
        return String::new();
    }
    from_wide(&s.gdi)
}

/// Monitors that are plugged in but not on the desktop. The `\\.\DISPLAYn` given to each is
/// a free output it could use; Windows only fixes the real one when the monitor is turned on.
fn detached() -> Vec<MonitorDesc> {
    let Ok((paths, _)) = query(QDC_ALL_PATHS) else { return Vec::new() };
    let active = |p: &PathInfo| p.flags & PATH_ACTIVE != 0;
    let on_targets: std::collections::HashSet<(LUID, u32)> =
        paths.iter().filter(|p| active(p)).map(|p| (p.target.adapter, p.target.id)).collect();
    let on_sources: std::collections::HashSet<(LUID, u32)> =
        paths.iter().filter(|p| active(p)).map(|p| (p.source.adapter, p.source.id)).collect();
    let mut out: Vec<MonitorDesc> = Vec::new();
    let mut used_gdi = std::collections::HashSet::new();
    for p in &paths {
        let target = (p.target.adapter, p.target.id);
        if active(p) || p.target.available == 0 || on_targets.contains(&target) {
            continue;
        }
        let (key, friendly) = target_name(p.target.adapter, p.target.id);
        if key.is_empty() || out.iter().any(|d| d.key == key) {
            continue;
        }
        let gdi = paths
            .iter()
            .filter(|q| (q.target.adapter, q.target.id) == target && !on_sources.contains(&(q.source.adapter, q.source.id)))
            .map(|q| source_gdi(q.source.adapter, q.source.id))
            .find(|g| !g.is_empty() && !used_gdi.contains(g))
            .unwrap_or_default();
        used_gdi.insert(gdi.clone());
        let mut desc = MonitorDesc {
            win_number: display_number(&gdi),
            gdi_name: gdi,
            off: true,
            key,
            ..Default::default()
        };
        if let Some((path, hw, short)) = parse_interface(&desc.key) {
            desc.instance_path = path;
            desc.hardware_id = hw;
            desc.instance_short = short;
        }
        if let Some(e) = read_edid(&desc.instance_path).as_deref().and_then(crate::edid::parse) {
            desc.manufacturer = e.manufacturer;
            desc.product = e.product;
            desc.serial_number = e.serial_number;
            desc.serial_text = e.serial_text.unwrap_or_default();
            desc.year = e.year;
            if let Some(nm) = e.name {
                desc.name = nm;
            }
        }
        if desc.name.is_empty() {
            desc.name = if friendly.is_empty() { "Monitor".into() } else { friendly };
        }
        out.push(desc);
    }
    out
}

/// Where and how a screen sits on the desktop, kept so it can come back the same way.
#[derive(Clone, Copy, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub freq: u32,
    pub orientation: u32,
}

/// Current placement of a screen that is on the desktop.
pub fn placement(gdi: &str) -> Option<Placement> {
    let dm = current_devmode(gdi)?;
    (dm.dmPelsWidth > 0).then_some(Placement {
        x: dm.dmPositionX,
        y: dm.dmPositionY,
        width: dm.dmPelsWidth,
        height: dm.dmPelsHeight,
        freq: dm.dmDisplayFrequency,
        orientation: dm.dmDisplayOrientation,
    })
}

/// Queues moving an output that is on to `at`. Nothing changes until `commit`.
fn stage_place(gdi: &str, at: Placement) -> Result<(), String> {
    let mut dm = zero_devmode();
    dm.dmPositionX = at.x;
    dm.dmPositionY = at.y;
    dm.dmPelsWidth = at.width;
    dm.dmPelsHeight = at.height;
    dm.dmDisplayOrientation = at.orientation;
    dm.dmFields = DM_POSITION | DM_PELSWIDTH | DM_PELSHEIGHT | DM_DISPLAYORIENTATION;
    if at.freq > 1 {
        dm.dmDisplayFrequency = at.freq;
        dm.dmFields |= DM_DISPLAYFREQUENCY;
    }
    let g = wide(gdi);
    change_result(unsafe { ChangeDisplaySettingsExW(g.as_ptr(), &dm, 0, CDS_UPDATEREGISTRY | CDS_NORESET, std::ptr::null()) })
}

/// Applies every queued change at once.
fn commit() -> Result<(), String> {
    change_result(unsafe { ChangeDisplaySettingsExW(std::ptr::null(), std::ptr::null(), 0, 0, std::ptr::null()) })
}

/// One monitor (by unique key) to turn on, with where to put it, or off.
pub enum Power {
    On(String, Option<Placement>),
    Off(String),
}

/// Turns several monitors on/off in one go. Returns the monitors (key, or "" for the whole
/// change) that failed, with the reason.
pub fn set_power(changes: &[Power]) -> Vec<(String, String)> {
    let (paths, modes) = match query(QDC_ALL_PATHS) {
        Ok(x) => x,
        Err(e) => return vec![(String::new(), e)],
    };
    let key_of = |p: &PathInfo| target_name(p.target.adapter, p.target.id).0;
    let off: std::collections::HashSet<&str> =
        changes.iter().filter_map(|c| if let Power::Off(k) = c { Some(k.as_str()) } else { None }).collect();
    let mut new: Vec<PathInfo> =
        paths.iter().filter(|p| p.flags & PATH_ACTIVE != 0 && !off.contains(key_of(p).as_str())).copied().collect();
    let mut used_sources: std::collections::HashSet<(LUID, u32)> = new.iter().map(|p| (p.source.adapter, p.source.id)).collect();
    let mut failed = Vec::new();
    let mut place = Vec::new();
    for c in changes {
        let Power::On(key, at) = c else { continue };
        if new.iter().any(|p| &key_of(p) == key) {
            continue;
        }
        let free = paths.iter().find(|p| {
            p.flags & PATH_ACTIVE == 0
                && p.target.available != 0
                && !used_sources.contains(&(p.source.adapter, p.source.id))
                && &key_of(p) == key
        });
        match free {
            Some(p) => {
                let mut q = *p;
                q.flags |= PATH_ACTIVE;
                q.source.mode_idx = MODE_IDX_INVALID;
                q.target.mode_idx = MODE_IDX_INVALID;
                used_sources.insert((q.source.adapter, q.source.id));
                new.push(q);
                if let Some(at) = at.filter(|a| a.width > 0) {
                    place.push((q.source.adapter, q.source.id, at));
                }
            }
            None => failed.push((key.clone(), "não há uma saída livre para este monitor".to_string())),
        }
    }
    if new.is_empty() {
        failed.push((String::new(), "tem de ficar pelo menos um ecrã ligado".into()));
        return failed;
    }
    let flags = SDC_APPLY | SDC_USE_SUPPLIED_DISPLAY_CONFIG | SDC_ALLOW_CHANGES | SDC_SAVE_TO_DATABASE;
    let r = unsafe { SetDisplayConfig(new.len() as u32, new.as_ptr(), modes.len() as u32, modes.as_ptr(), flags) };
    if r != 0 {
        failed.push((String::new(), format!("o Windows recusou a configuração (código {r})")));
        return failed;
    }
    // Put the screens that came back where they were.
    if !place.is_empty() {
        for (adapter, id, at) in place {
            let gdi = source_gdi(adapter, id);
            if let Err(e) = stage_place(&gdi, at) {
                failed.push((String::new(), e));
            }
        }
        if let Err(e) = commit() {
            failed.push((String::new(), e));
        }
    }
    failed
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn iface_parse() {
        let (p, hw, s) = parse_interface(r"\\?\DISPLAY#LEN66F2#5&2b6e6f2&0&UID4352#{e6f07b5f-ee97-4a90-b076-33f57bf4eaa7}").unwrap();
        assert_eq!(p, r"DISPLAY\LEN66F2\5&2b6e6f2&0&UID4352");
        assert_eq!(hw, "LEN66F2");
        assert_eq!(s, "UID4352");
        assert_eq!(display_number(r"\\.\DISPLAY12"), 12);
        assert_eq!(std::mem::size_of::<DEVMODEW>(), 220);
    }

    #[test]
    fn ccd_sizes() {
        assert_eq!(std::mem::size_of::<PathInfo>(), 72);
        assert_eq!(std::mem::size_of::<CcdMode>(), 64);
        assert_eq!(std::mem::size_of::<TargetName>(), 420);
        assert_eq!(std::mem::size_of::<SourceName>(), 84);
    }

    /// Real hardware: turns one monitor (by unique key) on then off again, or off then on.
    /// PowerShell: `$env:MCC_POWER_TEST = "<key>"; cargo test -- --ignored power_roundtrip --nocapture`
    #[test]
    #[ignore]
    fn power_roundtrip() {
        let key = std::env::var("MCC_POWER_TEST").expect("MCC_POWER_TEST").to_uppercase();
        let is_on = || enumerate().iter().any(|(d, _)| d.key == key && !d.off);
        let gdi_of = || enumerate().into_iter().find(|(d, _)| d.key == key).map(|(d, _)| d.gdi_name);
        let was_on = is_on();
        println!("{key}: on={was_on} gdi={:?}", gdi_of());
        let before = gdi_of().and_then(|g| placement(&g));
        let first = if was_on { Power::Off(key.clone()) } else { Power::On(key.clone(), None) };
        let failed = set_power(&[first]);
        assert!(failed.is_empty(), "{failed:?}");
        std::thread::sleep(std::time::Duration::from_secs(3));
        assert_eq!(is_on(), !was_on, "first change did not hit this monitor");
        println!("after first: gdi={:?} place={:?}", gdi_of(), gdi_of().and_then(|g| placement(&g)));
        let back = if was_on { Power::On(key.clone(), before) } else { Power::Off(key.clone()) };
        let failed = set_power(&[back]);
        assert!(failed.is_empty(), "{failed:?}");
        std::thread::sleep(std::time::Duration::from_secs(3));
        assert_eq!(is_on(), was_on, "second change did not hit this monitor");
        if was_on {
            assert_eq!(gdi_of().and_then(|g| placement(&g)), before);
        }
    }
}


