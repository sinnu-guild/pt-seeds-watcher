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

#[link(name = "gdi32")]
extern "system" {
    fn CreateCompatibleDC(hdc: HDC) -> HDC;
    fn CreateCompatibleBitmap(hdc: HDC, width: i32, height: i32) -> HGDIOBJ;
    fn SelectObject(hdc: HDC, obj: HGDIOBJ) -> HGDIOBJ;
    fn GetDIBits(
        hdc: HDC,
        bitmap: HGDIOBJ,
        start: u32,
        lines: u32,
        bits: *mut c_void,
        info: *mut BITMAPINFO,
        usage: u32,
    ) -> i32;
    fn DeleteObject(obj: HGDIOBJ) -> BOOL;
    fn DeleteDC(hdc: HDC) -> BOOL;
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

/// A captured picture of the window's client area, as RGBA bytes.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

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

/// Captures the window's client area (no title bar or borders).
pub fn capture(hwnd: isize) -> Result<Frame, String> {
    unsafe {
        if IsIconic(hwnd) != 0 {
            return Err("the game window is minimized; Windows doesn't draw minimized windows, \
                        so leave it open behind other windows or on another desktop"
                .into());
        }
        let mut rect = RECT::default();
        if GetClientRect(hwnd, &mut rect) == 0 {
            return Err("couldn't read the game window's size (did it just close?)".into());
        }
        let (w, h) = (rect.right - rect.left, rect.bottom - rect.top);
        if w <= 0 || h <= 0 {
            return Err(format!("the game window has no visible area ({w}x{h})"));
        }

        let screen_dc = GetDC(0);
        let mem_dc = CreateCompatibleDC(screen_dc);
        let bitmap = CreateCompatibleBitmap(screen_dc, w, h);
        let previous = SelectObject(mem_dc, bitmap);

        let printed = PrintWindow(hwnd, mem_dc, PW_CLIENTONLY | PW_RENDERFULLCONTENT);

        // GetDIBits wants the bitmap not selected into any DC.
        SelectObject(mem_dc, previous);

        let mut info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: w,
                biHeight: -h, // negative = rows from top to bottom
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB,
                biSizeImage: 0,
                biXPelsPerMeter: 0,
                biYPelsPerMeter: 0,
                biClrUsed: 0,
                biClrImportant: 0,
            },
            bmiColors: [0],
        };
        let mut bgra = vec![0u8; w as usize * h as usize * 4];
        let lines = GetDIBits(
            mem_dc,
            bitmap,
            0,
            h as u32,
            bgra.as_mut_ptr() as *mut c_void,
            &mut info,
            DIB_RGB_COLORS,
        );

        DeleteObject(bitmap);
        DeleteDC(mem_dc);
        ReleaseDC(0, screen_dc);

        if printed == 0 {
            return Err("Windows refused to draw the game window (PrintWindow failed)".into());
        }
        if lines == 0 {
            return Err("couldn't read the captured pixels (GetDIBits failed)".into());
        }

        // BGRA -> RGBA, fully opaque.
        for px in bgra.chunks_exact_mut(4) {
            px.swap(0, 2);
            px[3] = 255;
        }
        Ok(Frame {
            width: w as u32,
            height: h as u32,
            rgba: bgra,
        })
    }
}

