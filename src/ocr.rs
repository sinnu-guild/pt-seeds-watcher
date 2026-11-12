//! Reading text off the game window with Windows' built-in text recognition
//! (`Windows.Media.Ocr`, part of Windows 10 and 11).
//!
//! Only the left half of the window is read: that's where the shop's seed
//! list is. The right half shows the selected seed's details, whose name
//! would otherwise look like a list row.
//!
//! The picture is turned gray, its contrast stretched (the "Out of Stock"
//! text is pale orange on cream) and scaled so the window is about 1440
//! pixels tall. Without the upscaling the outlined "Stock: 1" text isn't read.

use image::{imageops, GrayImage};
use windows::core::HSTRING;
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapAlphaMode, BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Security::Cryptography::CryptographicBuffer;
use windows::Win32::System::WinRT::{RoInitialize, RO_INIT_MULTITHREADED};

use crate::capture::Frame;

/// The window height the picture is scaled to before reading.
const WORK_HEIGHT: f32 = 1440.0;

/// One line of text, with its box in game-window pixels.
#[derive(Clone, Debug)]
pub struct Line {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Must be called once on each thread that reads text, before `read`.
pub fn init_thread() {
    // An error just means this thread was already set up.
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
}

fn engine() -> Result<OcrEngine, String> {
    let english = Language::CreateLanguage(&HSTRING::from("en-US")).map_err(|e| e.to_string())?;
    OcrEngine::TryCreateFromLanguage(&english)
        .or_else(|_| OcrEngine::TryCreateFromUserProfileLanguages())
        .map_err(|_| {
            "Windows' English text recognition isn't installed. Add English in Windows Settings > \
             Time & language > Language & region, then restart the app."
                .to_string()
        })
}

/// The left half of the frame, gray, contrast-stretched and scaled up.
fn prepare(frame: &Frame) -> (GrayImage, f32) {
    let half_w = (frame.width / 2).max(1);
    let mut gray = GrayImage::new(half_w, frame.height);
    for (x, y, px) in gray.enumerate_pixels_mut() {
        let i = ((y * frame.width + x) * 4) as usize;
        let p = &frame.rgba[i..i + 3];
        px.0[0] = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) as u8;
    }
    stretch_contrast(&mut gray);
    let scale = (WORK_HEIGHT / frame.height as f32).clamp(1.0, 4.0);
    if scale > 1.0 {
        let (w, h) = ((half_w as f32 * scale) as u32, (frame.height as f32 * scale) as u32);
        gray = imageops::resize(&gray, w, h, imageops::FilterType::Lanczos3);
    }
    (gray, scale)
}

/// Spreads brightness over the full 0-255 range, ignoring the darkest and
/// brightest 1% of pixels.
fn stretch_contrast(img: &mut GrayImage) {
    let mut hist = [0u32; 256];
    for p in img.pixels() {
        hist[p.0[0] as usize] += 1;
    }
    let cut = (img.width() * img.height()) / 100;
    let (mut lo, mut seen) = (0usize, 0u32);
    while lo < 255 && seen + hist[lo] <= cut {
        seen += hist[lo];
        lo += 1;
    }
    let (mut hi, mut seen) = (255usize, 0u32);
    while hi > 0 && seen + hist[hi] <= cut {
        seen += hist[hi];
        hi -= 1;
    }
    if hi <= lo {
        return;
    }
    let range = (hi - lo) as f32;
    for p in img.pixels_mut() {
        let v = (p.0[0] as f32 - lo as f32) / range * 255.0;
        p.0[0] = v.clamp(0.0, 255.0) as u8;
    }
}

/// Reads the text in the seed-list half of the frame.
pub fn read(frame: &Frame) -> Result<Vec<Line>, String> {
    let (gray, scale) = prepare(frame);
    let engine = engine()?;
    let max = OcrEngine::MaxImageDimension().unwrap_or(10000);
    if gray.width() > max || gray.height() > max {
        return Err(format!("the game window is too big to read ({}x{})", frame.width, frame.height));
    }

    let mut bgra = Vec::with_capacity(gray.len() * 4);
    for p in gray.pixels() {
        let v = p.0[0];
        bgra.extend_from_slice(&[v, v, v, 255]);
    }
    let err = |e: windows::core::Error| format!("text recognition failed: {e}");
    let buffer = CryptographicBuffer::CreateFromByteArray(&bgra).map_err(err)?;
    let bitmap = SoftwareBitmap::CreateCopyWithAlphaFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        gray.width() as i32,
        gray.height() as i32,
        BitmapAlphaMode::Premultiplied,
    )
    .map_err(err)?;
    let result = engine.RecognizeAsync(&bitmap).map_err(err)?.join().map_err(err)?;

    let mut out = Vec::new();
    let lines = result.Lines().map_err(err)?;
    for i in 0..lines.Size().map_err(err)? {
        let line = lines.GetAt(i).map_err(err)?;
        let words = line.Words().map_err(err)?;
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for j in 0..words.Size().map_err(err)? {
            let r = words.GetAt(j).map_err(err)?.BoundingRect().map_err(err)?;
            x0 = x0.min(r.X);
            y0 = y0.min(r.Y);
            x1 = x1.max(r.X + r.Width);
            y1 = y1.max(r.Y + r.Height);
        }
        if x0 > x1 {
            continue;
        }
        out.push(Line {
            text: line.Text().map_err(err)?.to_string(),
            x: x0 / scale,
            y: y0 / scale,
            w: (x1 - x0) / scale,
            h: (y1 - y0) / scale,
        });
    }
    Ok(out)
}
