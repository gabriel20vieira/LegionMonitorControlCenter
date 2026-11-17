//! "Identificar": shows a big number on every monitor for a few seconds,
//! like Windows Settings does, drawn with plain Win32 so it lands on the
//! right screen in physical pixels.
#![allow(non_snake_case, clippy::upper_case_acronyms)]

use crate::win::{wide, HWND, RECT};
use std::sync::Mutex;

type WNDPROC = unsafe extern "system" fn(HWND, u32, usize, isize) -> isize;

#[repr(C)]
struct WNDCLASSEXW {
    cbSize: u32,
    style: u32,
    lpfnWndProc: WNDPROC,
    cbClsExtra: i32,
    cbWndExtra: i32,
    hInstance: isize,
    hIcon: isize,
    hCursor: isize,
    hbrBackground: isize,
    lpszMenuName: *const u16,
    lpszClassName: *const u16,
    hIconSm: isize,
}

#[repr(C)]
struct PAINTSTRUCT {
    hdc: isize,
    fErase: i32,
    rcPaint: RECT,
    fRestore: i32,
    fIncUpdate: i32,
    rgbReserved: [u8; 32],
}

#[repr(C)]
struct MSG {
    hwnd: HWND,
    message: u32,
    wParam: usize,
    lParam: isize,
    time: u32,
    pt: [i32; 2],
    extra: [u32; 4],
}

#[link(name = "user32")]
extern "system" {
    fn RegisterClassExW(wc: *const WNDCLASSEXW) -> u16;
    fn CreateWindowExW(ex: u32, cls: *const u16, name: *const u16, style: u32, x: i32, y: i32, w: i32, h: i32, parent: HWND, menu: isize, inst: isize, param: *const std::ffi::c_void) -> HWND;
    fn DefWindowProcW(h: HWND, m: u32, w: usize, l: isize) -> isize;
    fn GetMessageW(msg: *mut MSG, h: HWND, a: u32, b: u32) -> i32;
    fn TranslateMessage(msg: *const MSG) -> i32;
    fn DispatchMessageW(msg: *const MSG) -> isize;
    fn SetTimer(h: HWND, id: usize, ms: u32, f: isize) -> usize;
    fn DestroyWindow(h: HWND) -> i32;
    fn PostQuitMessage(code: i32);
    fn BeginPaint(h: HWND, ps: *mut PAINTSTRUCT) -> isize;
    fn EndPaint(h: HWND, ps: *const PAINTSTRUCT) -> i32;
    fn FillRect(hdc: isize, r: *const RECT, br: isize) -> i32;
    fn GetClientRect(h: HWND, r: *mut RECT) -> i32;
    fn DrawTextW(hdc: isize, s: *const u16, n: i32, r: *mut RECT, fmt: u32) -> i32;
}

#[link(name = "gdi32")]
extern "system" {
    fn CreateSolidBrush(c: u32) -> isize;
    fn DeleteObject(o: isize) -> i32;
    fn CreateFontW(h: i32, w: i32, esc: i32, ori: i32, weight: i32, it: u32, ul: u32, so: u32, cs: u32, op: u32, cp: u32, q: u32, pf: u32, face: *const u16) -> isize;
    fn SelectObject(hdc: isize, o: isize) -> isize;
    fn SetTextColor(hdc: isize, c: u32) -> u32;
    fn SetBkMode(hdc: isize, m: i32) -> i32;
}

#[link(name = "kernel32")]
extern "system" {
    fn GetModuleHandleW(name: *const u16) -> isize;
}

const WM_PAINT: u32 = 0x000F;
const WM_TIMER: u32 = 0x0113;
const WS_POPUP: u32 = 0x8000_0000;
const WS_VISIBLE: u32 = 0x1000_0000;
const WS_EX_TOPMOST: u32 = 0x0000_0008;
const WS_EX_TOOLWINDOW: u32 = 0x0000_0080;
const WS_EX_NOACTIVATE: u32 = 0x0800_0000;
const DT_CENTER: u32 = 0x1;
const DT_VCENTER: u32 = 0x4;
const DT_SINGLELINE: u32 = 0x20;
const DT_WORDBREAK: u32 = 0x10;

/// (hwnd, big number, caption, highlighted)
static LABELS: Mutex<Vec<(HWND, String, String, bool)>> = Mutex::new(Vec::new());
static BUSY: Mutex<bool> = Mutex::new(false);

const fn rgb(r: u8, g: u8, b: u8) -> u32 {
    (r as u32) | ((g as u32) << 8) | ((b as u32) << 16)
}

unsafe extern "system" fn wnd_proc(h: HWND, m: u32, w: usize, l: isize) -> isize {
    match m {
        WM_PAINT => {
            let mut ps: PAINTSTRUCT = std::mem::zeroed();
            let hdc = BeginPaint(h, &mut ps);
            let mut rc = RECT::default();
            GetClientRect(h, &mut rc);
            let entry = LABELS.lock().ok().and_then(|v| v.iter().find(|e| e.0 == h).cloned());
            let (num, cap, hi) = entry.map(|e| (e.1, e.2, e.3)).unwrap_or_default();
            let bg = CreateSolidBrush(if hi { rgb(0x49, 0x90, 0xff) } else { rgb(0x1f, 0x2a, 0x42) });
            FillRect(hdc, &rc, bg);
            DeleteObject(bg);
            SetBkMode(hdc, 1);
            SetTextColor(hdc, rgb(255, 255, 255));
            let face = wide("Segoe UI");
            let big = CreateFontW(-170, 0, 0, 0, 700, 0, 0, 0, 1, 0, 0, 5, 0, face.as_ptr());
            let small = CreateFontW(-20, 0, 0, 0, 600, 0, 0, 0, 1, 0, 0, 5, 0, face.as_ptr());
            let old = SelectObject(hdc, big);
            let mut top = RECT { bottom: rc.bottom - 64, ..rc };
            let n = wide(&num);
            DrawTextW(hdc, n.as_ptr(), -1, &mut top, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
            SelectObject(hdc, small);
            let mut bottom = RECT { top: rc.bottom - 70, left: rc.left + 8, right: rc.right - 8, ..rc };
            let c = wide(&cap);
            DrawTextW(hdc, c.as_ptr(), -1, &mut bottom, DT_CENTER | DT_WORDBREAK);
            SelectObject(hdc, old);
            DeleteObject(big);
            DeleteObject(small);
            EndPaint(h, &ps);
            0
        }
        WM_TIMER => {
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(h, m, w, l),
    }
}

/// Show `(rect, number, caption, highlighted)` on each monitor for `ms` milliseconds.
pub fn identify(items: Vec<(RECT, String, String, bool)>, ms: u32) {
    {
        let mut b = BUSY.lock().unwrap();
        if *b {
            return;
        }
        *b = true;
    }
    std::thread::spawn(move || unsafe {
        let inst = GetModuleHandleW(std::ptr::null());
        let cls = wide("LegionMonitorIdentify");
        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: 0,
            lpfnWndProc: wnd_proc,
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: inst,
            hIcon: 0,
            hCursor: 0,
            hbrBackground: 0,
            lpszMenuName: std::ptr::null(),
            lpszClassName: cls.as_ptr(),
            hIconSm: 0,
        };
        RegisterClassExW(&wc); // fails harmlessly when already registered
        let title = wide("");
        let mut hwnds = Vec::new();
        for (r, num, cap, hi) in items {
            let (w, h) = (260, 280);
            let hwnd = CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                cls.as_ptr(),
                title.as_ptr(),
                WS_POPUP,
                r.left + 48,
                r.top + 48,
                w,
                h,
                0,
                0,
                inst,
                std::ptr::null(),
            );
            if hwnd != 0 {
                LABELS.lock().unwrap().push((hwnd, num, cap, hi));
                hwnds.push(hwnd);
            }
        }
        // Show only after labels are registered (WS_VISIBLE later avoids an unlabelled first paint).
        #[link(name = "user32")]
        extern "system" {
            fn ShowWindow(h: HWND, cmd: i32) -> i32;
            fn UpdateWindow(h: HWND) -> i32;
        }
        for &h in &hwnds {
            ShowWindow(h, 4); // SW_SHOWNOACTIVATE
            UpdateWindow(h);
        }
        let _ = WS_VISIBLE;
        if let Some(&first) = hwnds.first() {
            SetTimer(first, 1, ms, 0);
            let mut msg: MSG = std::mem::zeroed();
            while GetMessageW(&mut msg, 0, 0, 0) > 0 {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        for h in hwnds {
            DestroyWindow(h);
        }
        LABELS.lock().unwrap().clear();
        *BUSY.lock().unwrap() = false;
    });
}
