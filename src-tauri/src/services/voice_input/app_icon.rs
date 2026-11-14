use base64::Engine;
use image::{DynamicImage, ImageFormat};
use std::io::Cursor;

const ICON_SIZE: u32 = 64;

pub(super) fn application_icon(target: &str) -> Option<String> {
    #[cfg(windows)]
    {
        encode_image(desktop_windows::read_path(target)?)
    }
    #[cfg(target_os = "macos")]
    {
        objc2::rc::autoreleasepool(|_| {
            let path = objc2_foundation::NSString::from_str(target);
            let icon = objc2_app_kit::NSWorkspace::sharedWorkspace().iconForFile(&path);
            encode_image(render_macos_icon(&icon)?)
        })
    }
    #[cfg(target_os = "linux")]
    {
        desktop_linux::read_names(&[target.strip_suffix(".desktop").unwrap_or(target).into()])
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(super) enum Target {
    Window(u64),
    #[cfg(target_os = "linux")]
    Desktop(super::wayland_state::DesktopApp),
}

pub(super) struct Monitor {
    #[cfg(target_os = "linux")]
    wayland: Option<super::wayland::Tracker>,
}

impl Monitor {
    pub fn new() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            wayland: super::native::wayland().then(super::wayland::Tracker::start),
        }
    }

    pub fn current(&self) -> Option<Target> {
        #[cfg(target_os = "linux")]
        if let Some(tracker) = &self.wayland {
            return tracker.current().map(Target::Desktop);
        }
        super::native::foreground().map(Target::Window)
    }
}

pub(super) fn cache_key(target: &Target) -> Option<String> {
    match target {
        #[cfg(windows)]
        Target::Window(window) => desktop_windows::cache_key(*window),
        #[cfg(target_os = "macos")]
        Target::Window(pid) => {
            let app = objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(
                i32::try_from(*pid).ok()?,
            )?;
            let identity = app.bundleIdentifier()?.to_string();
            Some(format!("bundle:{identity}"))
        }
        #[cfg(target_os = "linux")]
        Target::Window(window) => desktop_linux::cache_key(*window),
        #[cfg(target_os = "linux")]
        Target::Desktop(app) => Some(format!("desktop:{}", app.app_id)),
    }
}

pub(super) fn read_target(target: &Target) -> Option<String> {
    match target {
        Target::Window(window) => read(*window),
        #[cfg(target_os = "linux")]
        Target::Desktop(app) => desktop_linux::read_app_id(&app.app_id),
    }
}

#[cfg(windows)]
#[path = "app_icon_desktop_windows.rs"]
mod desktop_windows;

#[cfg(target_os = "linux")]
#[path = "app_icon_desktop_linux.rs"]
mod desktop_linux;

#[cfg(all(test, windows))]
#[path = "app_icon_tests.rs"]
mod tests;

pub fn read(window: u64) -> Option<String> {
    #[cfg(target_os = "linux")]
    if let Some(icon) = desktop_linux::read(window) {
        return Some(icon);
    }
    encode_image(native_icon(window)?)
}

fn encode_image(image: DynamicImage) -> Option<String> {
    // Displayed at 32 CSS pixels; retain enough detail for a 2x display.
    let image = image.resize(ICON_SIZE, ICON_SIZE, image::imageops::FilterType::Lanczos3);
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, ImageFormat::Png).ok()?;
    Some(format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(png.into_inner())
    ))
}

#[cfg(windows)]
fn native_icon(window: u64) -> Option<DynamicImage> {
    desktop_windows::read(window)
        .or_else(|| window_icon(window, true).and_then(draw_icon))
        .or_else(|| window_icon(window, false).and_then(draw_icon))
}

#[cfg(windows)]
fn window_icon(window: u64, large: bool) -> Option<windows::Win32::UI::WindowsAndMessaging::HICON> {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetClassLongPtrW, SendMessageTimeoutW, GCLP_HICON, GCLP_HICONSM, HICON, ICON_BIG,
        ICON_SMALL2, SMTO_ABORTIFHUNG, WM_GETICON,
    };
    // Both window and class icons are borrowed. Never destroy them.
    unsafe {
        let window = HWND(window as *mut _);
        let mut icon = 0;
        let _ = SendMessageTimeoutW(
            window,
            WM_GETICON,
            WPARAM(if large { ICON_BIG } else { ICON_SMALL2 } as usize),
            LPARAM((ICON_SIZE * 96 / 32) as isize),
            SMTO_ABORTIFHUNG,
            50,
            Some(&mut icon),
        );
        if icon == 0 {
            icon = GetClassLongPtrW(window, if large { GCLP_HICON } else { GCLP_HICONSM });
        }
        (icon != 0).then_some(HICON(icon as *mut _))
    }
}

#[cfg(windows)]
fn draw_icon(icon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<DynamicImage> {
    use windows::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GdiFlush, GetObjectW,
        SelectObject, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
    };
    use windows::Win32::UI::WindowsAndMessaging::{DrawIconEx, GetIconInfo, DI_NORMAL, ICONINFO};
    unsafe {
        let mut source = ICONINFO::default();
        GetIconInfo(icon, &mut source).ok()?;
        let monochrome = source.hbmColor.0.is_null();
        let mut dimensions = BITMAP::default();
        let measured = GetObjectW(
            HGDIOBJ(if monochrome {
                source.hbmMask.0
            } else {
                source.hbmColor.0
            }),
            std::mem::size_of::<BITMAP>() as i32,
            Some((&mut dimensions as *mut BITMAP).cast()),
        );
        // GetIconInfo creates these bitmaps even for a borrowed HICON.
        let _ = DeleteObject(HGDIOBJ(source.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(source.hbmMask.0));
        let width = u32::try_from(dimensions.bmWidth).ok()?.min(ICON_SIZE);
        let height = u32::try_from(dimensions.bmHeight / if monochrome { 2 } else { 1 })
            .ok()?
            .min(ICON_SIZE);
        if measured == 0 || width == 0 || height == 0 {
            return None;
        }
        let dc = CreateCompatibleDC(None);
        if dc.0.is_null() {
            return None;
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        let bitmap = match CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            Ok(bitmap) => bitmap,
            Err(_) => {
                let _ = DeleteDC(dc);
                return None;
            }
        };
        let previous = SelectObject(dc, HGDIOBJ(bitmap.0));
        let bytes = (width * height * 4) as usize;
        std::ptr::write_bytes(bits.cast::<u8>(), 0, bytes);
        let drawn = DrawIconEx(
            dc,
            0,
            0,
            icon,
            width as i32,
            height as i32,
            0,
            None,
            DI_NORMAL,
        );
        let _ = GdiFlush();
        let pixels = std::slice::from_raw_parts(bits.cast::<u8>(), bytes).to_vec();
        let _ = SelectObject(dc, previous);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        let _ = DeleteDC(dc);
        drawn.ok()?;
        bgra_image(width, height, pixels, true)
    }
}

#[cfg(windows)]
fn bgra_image(
    width: u32,
    height: u32,
    mut pixels: Vec<u8>,
    premultiplied: bool,
) -> Option<DynamicImage> {
    let opaque = pixels.chunks_exact(4).all(|pixel| pixel[3] == 0);
    for pixel in pixels.chunks_exact_mut(4) {
        pixel.swap(0, 2);
        if opaque {
            pixel[3] = 255;
        } else if premultiplied && pixel[3] > 0 && pixel[3] < 255 {
            // GDI blends premultiplied BGRA; PNG needs straight alpha.
            let alpha = u16::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    image::RgbaImage::from_raw(width, height, pixels).map(DynamicImage::ImageRgba8)
}

#[cfg(target_os = "macos")]
fn native_icon(pid: u64) -> Option<DynamicImage> {
    let pid = i32::try_from(pid).ok()?;
    // The icon thread has no run loop to drain AppKit's autoreleased objects.
    objc2::rc::autoreleasepool(|_| {
        let app =
            objc2_app_kit::NSRunningApplication::runningApplicationWithProcessIdentifier(pid)?;
        let icon = app.icon()?;
        render_macos_icon(&icon)
    })
}

/// Draws the icon into an 8-bit RGBA bitmap at the island's size. Its TIFF
/// representation can be a 16-bit floating-point image that `image` cannot decode
/// (macOS 27), and is tens of megabytes for a 64px icon.
#[cfg(target_os = "macos")]
fn render_macos_icon(icon: &objc2_app_kit::NSImage) -> Option<DynamicImage> {
    use objc2::AnyThread;
    use objc2_app_kit::{
        NSBitmapImageFileType, NSBitmapImageRep, NSCompositingOperation, NSDeviceRGBColorSpace,
        NSGraphicsContext,
    };
    use objc2_foundation::{NSDictionary, NSPoint, NSRect, NSSize};
    let side = ICON_SIZE as isize;
    // SAFETY: a null plane pointer asks AppKit to allocate the pixel buffer.
    let bitmap = unsafe {
        NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(),
            side,
            side,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            0,
            0,
        )
    }?;
    let context = NSGraphicsContext::graphicsContextWithBitmapImageRep(&bitmap)?;
    // The current context is per thread; restore this thread's previous one.
    NSGraphicsContext::saveGraphicsState_class();
    NSGraphicsContext::setCurrentContext(Some(&context));
    let size = f64::from(ICON_SIZE);
    icon.drawInRect_fromRect_operation_fraction(
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(size, size)),
        NSRect::ZERO,
        NSCompositingOperation::Copy,
        1.0,
    );
    context.flushGraphics();
    NSGraphicsContext::restoreGraphicsState_class();
    // SAFETY: an empty property dictionary is valid for PNG output.
    let png = unsafe {
        bitmap.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
    }?;
    image::load_from_memory(&png.to_vec()).ok()
}

#[cfg(target_os = "linux")]
fn native_icon(window: u64) -> Option<DynamicImage> {
    use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
    if super::native::wayland() {
        return None;
    }
    let (connection, _) = x11rb::connect(None).ok()?;
    let atom = connection
        .intern_atom(false, b"_NET_WM_ICON")
        .ok()?
        .reply()
        .ok()?
        .atom;
    let property = connection
        .get_property(
            false,
            u32::try_from(window).ok()?,
            atom,
            AtomEnum::CARDINAL,
            0,
            262144,
        )
        .ok()?
        .reply()
        .ok()?;
    let values = property.value32()?.collect::<Vec<_>>();
    x11_icon(&values).map(DynamicImage::ImageRgba8)
}

#[cfg(any(target_os = "linux", test))]
pub(super) fn x11_icon(values: &[u32]) -> Option<image::RgbaImage> {
    let mut remaining = values;
    let mut best = None;
    let mut distance = (true, u32::MAX);
    while remaining.len() >= 2 {
        let (width, height) = (remaining[0], remaining[1]);
        let count = width.checked_mul(height)? as usize;
        let pixels = remaining.get(2..count.checked_add(2)?)?;
        if width > 0 && height > 0 && width <= 512 && height <= 512 {
            let next_distance = (
                width < ICON_SIZE || height < ICON_SIZE,
                width.abs_diff(ICON_SIZE) + height.abs_diff(ICON_SIZE),
            );
            if next_distance < distance {
                best = Some((width, height, pixels));
                distance = next_distance;
            }
        }
        remaining = &remaining[count + 2..];
    }
    let (width, height, pixels) = best?;
    let rgba = pixels
        .iter()
        .flat_map(|argb| {
            [
                (argb >> 16) as u8,
                (argb >> 8) as u8,
                *argb as u8,
                (argb >> 24) as u8,
            ]
        })
        .collect();
    image::RgbaImage::from_raw(width, height, rgba)
}

#[cfg(all(test, target_os = "macos"))]
mod macos_tests {
    use super::*;

    #[test]
    fn application_icons_render_as_8_bit_images_at_island_size() {
        // Finder's bundle icon has the same multi-resolution representations as a
        // running app's icon; its TIFF form is 16-bit float on macOS 27.
        let icon = objc2_app_kit::NSWorkspace::sharedWorkspace().iconForFile(
            &objc2_foundation::NSString::from_str("/System/Library/CoreServices/Finder.app"),
        );
        let image = render_macos_icon(&icon).expect("the icon renders");
        assert_eq!((image.width(), image.height()), (ICON_SIZE, ICON_SIZE));
        let visible = image
            .to_rgba8()
            .pixels()
            .filter(|pixel| pixel[3] > 0)
            .count();
        assert!(
            visible > (ICON_SIZE * ICON_SIZE / 2) as usize,
            "visible={visible}"
        );
        assert!(encode_image(image).is_some_and(|icon| icon.starts_with("data:image/png;base64,")));
    }
}
