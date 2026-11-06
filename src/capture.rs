//! Finding the game window and taking a picture of it.
//!
//! Uses Win32 `PrintWindow` with `PW_RENDERFULLCONTENT`, which asks the window
//! to draw itself into our bitmap. That works even when other windows cover
//! the game. It does NOT work when the game is minimized (Windows stops
//! drawing it). Whether it works for a window on another virtual desktop
//! depends on the game, so `snapshot` exists to check that.
//!
//! The handful of Win32 functions are declared by hand rather than through
//! the `windows` crate, so the code doesn't depend on that crate's changing API.
//! The program never sends input to the game: it only reads pixels.

#![allow(non_snake_case, non_camel_case_types, clippy::upper_case_acronyms)]

use std::ffi::c_void;

type HANDLE = isize;
type HWND = HANDLE;
type HDC = HANDLE;
type HGDIOBJ = HANDLE;
type BOOL = i32;

#[repr(C)]
#[derive(Default)]
struct RECT {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[repr(C)]
struct BITMAPINFOHEADER {
    biSize: u32,
    biWidth: i32,
    biHeight: i32,
    biPlanes: u16,
    biBitCount: u16,
    biCompression: u32,
    biSizeImage: u32,
    biXPelsPerMeter: i32,
    biYPelsPerMeter: i32,
    biClrUsed: u32,
    biClrImportant: u32,
}

#[repr(C)]
struct BITMAPINFO {
    bmiHeader: BITMAPINFOHEADER,
    bmiColors: [u32; 1],
}

#[repr(C)]
#[derive(Default)]
struct SYSTEMTIME {
    wYear: u16,
    wMonth: u16,
    wDayOfWeek: u16,
    wDay: u16,
    wHour: u16,
    wMinute: u16,
    wSecond: u16,
    wMilliseconds: u16,
}

type WNDENUMPROC = unsafe extern "system" fn(HWND, isize) -> BOOL;

#[link(name = "user32")]
extern "system" {
    fn EnumWindows(callback: WNDENUMPROC, lparam: isize) -> BOOL;
    fn GetWindowTextW(hwnd: HWND, buf: *mut u16, max_count: i32) -> i32;
    fn GetWindowTextLengthW(hwnd: HWND) -> i32;
    fn IsWindowVisible(hwnd: HWND) -> BOOL;
    fn IsIconic(hwnd: HWND) -> BOOL;
    fn GetClientRect(hwnd: HWND, rect: *mut RECT) -> BOOL;
    fn GetDC(hwnd: HWND) -> HDC;
    fn ReleaseDC(hwnd: HWND, hdc: HDC) -> i32;
    fn PrintWindow(hwnd: HWND, hdc: HDC, flags: u32) -> BOOL;
    fn SetProcessDpiAwarenessContext(value: HANDLE) -> BOOL;
    fn MessageBeep(kind: u32) -> BOOL;
}


#[link(name = "dwmapi")]
extern "system" {
    fn DwmGetWindowAttribute(hwnd: HWND, attr: u32, value: *mut c_void, size: u32) -> i32;
}


const PW_CLIENTONLY: u32 = 0x1;
const PW_RENDERFULLCONTENT: u32 = 0x2;
const DIB_RGB_COLORS: u32 = 0;
const BI_RGB: u32 = 0;
const DWMWA_CLOAKED: u32 = 14;
const DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2: HANDLE = -4;
const MB_ICONEXCLAMATION: u32 = 0x30;


pub struct WindowInfo {
    pub hwnd: isize,
    pub title: String,
    pub minimized: bool,
    /// True when Windows hides ("cloaks") the window, e.g. it's on another virtual desktop.
    pub cloaked: bool,
    pub width: i32,
    pub height: i32,
}


unsafe extern "system" fn collect_window(hwnd: HWND, lparam: isize) -> BOOL {
    let out = &mut *(lparam as *mut Vec<WindowInfo>);
    if IsWindowVisible(hwnd) == 0 {
        return 1;
    }
    let len = GetWindowTextLengthW(hwnd);
    if len <= 0 {
        return 1;
    }
    let mut buf = vec![0u16; len as usize + 1];
    let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
    let title = String::from_utf16_lossy(&buf[..n.max(0) as usize]);

    let mut rect = RECT::default();
    GetClientRect(hwnd, &mut rect);

    let mut cloaked: u32 = 0;
    let hr = DwmGetWindowAttribute(
        hwnd,
        DWMWA_CLOAKED,
        &mut cloaked as *mut u32 as *mut c_void,
        std::mem::size_of::<u32>() as u32,
    );

    out.push(WindowInfo {
        hwnd,
        title,
        minimized: IsIconic(hwnd) != 0,
        cloaked: hr == 0 && cloaked != 0,
        width: rect.right - rect.left,
        height: rect.bottom - rect.top,
    });
    1
}

/// All visible top-level windows that have a title.
pub fn list_windows() -> Vec<WindowInfo> {
    let mut out: Vec<WindowInfo> = Vec::new();
    unsafe {
        EnumWindows(collect_window, &mut out as *mut Vec<WindowInfo> as isize);
    }
    out
}

fn normalize(s: &str) -> String {
    s.chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

/// How well a window title matches what we're looking for, ignoring case and
/// spaces. 3 = exact, 2 = starts with, 1 = contains, 0 = no match.
pub fn title_rank(title: &str, wanted: &str) -> u8 {
    let (t, w) = (normalize(title), normalize(wanted));
    if w.is_empty() {
        0
    } else if t == w {
        3
    } else if t.starts_with(&w) {
        2
    } else if t.contains(&w) {
        1
    } else {
        0
    }
}

/// Finds the window that best matches `wanted`, preferring exact title matches
/// so a browser tab that mentions the game doesn't win over the game itself.
pub fn find_window(wanted: &str) -> Option<WindowInfo> {
    list_windows()
        .into_iter()
        .map(|w| (title_rank(&w.title, wanted), w))
        .filter(|(rank, _)| *rank > 0)
        .max_by_key(|(rank, w)| (*rank, w.width as i64 * w.height as i64))
        .map(|(_, w)| w)
}

