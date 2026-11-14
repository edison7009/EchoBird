use super::{bgra_image, DynamicImage, ICON_SIZE};
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use windows::core::{Interface, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, DeleteDC, DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HGDIOBJ,
};
use windows::Win32::Storage::Packaging::Appx::GetApplicationUserModelId;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, IPersistFile,
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, STGM_READ,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Shell::{
    FOLDERID_CommonPrograms, FOLDERID_Desktop, FOLDERID_Programs, FOLDERID_PublicDesktop,
    IShellItemImageFactory, IShellLinkW, SHCreateItemFromParsingName, SHGetKnownFolderPath,
    ShellLink, KF_FLAG_DEFAULT, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY,
};
use windows::Win32::UI::WindowsAndMessaging::GetWindowThreadProcessId;

struct Apartment(bool);

impl Apartment {
    fn enter() -> Self {
        Self(unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).is_ok() })
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        if self.0 {
            unsafe { CoUninitialize() }
        }
    }
}

fn identity(window: u64) -> Option<(Option<String>, Option<String>)> {
    unsafe {
        let mut pid = 0;
        GetWindowThreadProcessId(HWND(window as *mut _), Some(&mut pid));
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut identity = vec![0_u16; 512];
        let mut length = identity.len() as u32;
        let identity =
            GetApplicationUserModelId(process, &mut length, Some(PWSTR(identity.as_mut_ptr())))
                .is_ok()
                .then(|| String::from_utf16_lossy(&identity[..length.saturating_sub(1) as usize]));
        let mut path = vec![0_u16; 32_768];
        let mut length = path.len() as u32;
        let executable = QueryFullProcessImageNameW(
            process,
            PROCESS_NAME_WIN32,
            PWSTR(path.as_mut_ptr()),
            &mut length,
        )
        .is_ok()
        .then(|| String::from_utf16_lossy(&path[..length as usize]));
        let _ = CloseHandle(process);
        Some((identity, executable))
    }
}

pub(super) fn cache_key(window: u64) -> Option<String> {
    let (identity, executable) = identity(window)?;
    identity
        .map(|id| format!("app:{}", id.to_ascii_lowercase()))
        .or_else(|| executable.map(|path| format!("exe:{}", path.to_ascii_lowercase())))
}

pub(super) fn read(window: u64) -> Option<DynamicImage> {
    let _apartment = Apartment::enter();
    let (identity, executable) = identity(window)?;
    if let Some(icon) = identity.and_then(|id| shell_image(&format!("shell:AppsFolder\\{id}"))) {
        return Some(icon);
    }
    let executable = executable?;
    // These are the same registered shortcuts used by the desktop/start menu.
    // Never resolve or launch a link, and never inspect executable resources.
    for (folder, depth) in [
        (FOLDERID_Desktop, 0),
        (FOLDERID_PublicDesktop, 0),
        (FOLDERID_Programs, 5),
        (FOLDERID_CommonPrograms, 5),
    ] {
        let root = unsafe {
            let path = match SHGetKnownFolderPath(&folder, KF_FLAG_DEFAULT, None) {
                Ok(path) => path,
                Err(_) => continue,
            };
            let root = path.to_string().ok().map(PathBuf::from);
            CoTaskMemFree(Some(path.0.cast()));
            root
        };
        if let Some(icon) = root.and_then(|root| shortcut_icon(&root, &executable, depth)) {
            return Some(icon);
        }
    }
    None
}

fn shortcut_icon(directory: &Path, executable: &str, depth: u8) -> Option<DynamicImage> {
    for entry in std::fs::read_dir(directory).ok()?.flatten() {
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(_) => continue,
        };
        let path = entry.path();
        if kind.is_dir() && depth > 0 {
            if let Some(icon) = shortcut_icon(&path, executable, depth - 1) {
                return Some(icon);
            }
        } else if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("lnk"))
            && shortcut_matches(&path, executable)
        {
            if let Some(icon) = shell_image(path.to_str()?) {
                return Some(icon);
            }
        }
    }
    None
}

fn shortcut_matches(path: &Path, executable: &str) -> bool {
    let result = (|| -> windows::core::Result<bool> {
        unsafe {
            let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)?;
            let file: IPersistFile = link.cast()?;
            let path: Vec<_> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            file.Load(PCWSTR(path.as_ptr()), STGM_READ)?;
            let mut target = vec![0_u16; 32_768];
            link.GetPath(&mut target, std::ptr::null_mut(), 0)?;
            let length = target
                .iter()
                .position(|value| *value == 0)
                .unwrap_or(target.len());
            Ok(String::from_utf16_lossy(&target[..length]).eq_ignore_ascii_case(executable))
        }
    })();
    result.unwrap_or(false)
}

pub(super) fn read_path(path: &str) -> Option<DynamicImage> {
    let _apartment = Apartment::enter();
    shell_image(path)
}

fn shell_image(path: &str) -> Option<DynamicImage> {
    unsafe {
        let factory: IShellItemImageFactory =
            SHCreateItemFromParsingName(&HSTRING::from(path), None).ok()?;
        let bitmap = factory
            .GetImage(
                SIZE {
                    cx: ICON_SIZE as i32,
                    cy: ICON_SIZE as i32,
                },
                SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK,
            )
            .ok()?;
        let image = bitmap_image(bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        image
    }
}

fn bitmap_image(bitmap: HBITMAP) -> Option<DynamicImage> {
    unsafe {
        let mut dimensions = BITMAP::default();
        if GetObjectW(
            HGDIOBJ(bitmap.0),
            std::mem::size_of::<BITMAP>() as i32,
            Some((&mut dimensions as *mut BITMAP).cast()),
        ) == 0
        {
            return None;
        }
        let width = u32::try_from(dimensions.bmWidth).ok()?;
        let height = u32::try_from(dimensions.bmHeight).ok()?;
        if width == 0 || height == 0 || width > 512 || height > 512 {
            return None;
        }
        let dc = CreateCompatibleDC(None);
        if dc.0.is_null() {
            return None;
        }
        let mut info = BITMAPINFO {
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
        let mut pixels = vec![0_u8; (width * height * 4) as usize];
        let rows = GetDIBits(
            dc,
            bitmap,
            0,
            height,
            Some(pixels.as_mut_ptr().cast()),
            &mut info,
            DIB_RGB_COLORS,
        );
        let _ = DeleteDC(dc);
        if rows != height as i32 {
            return None;
        }
        // ICONONLY shell bitmaps carry straight alpha, unlike DrawIconEx's DIB.
        bgra_image(width, height, pixels, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::ico::{IcoEncoder, IcoFrame};

    #[test]
    fn desktop_shortcut_uses_its_registered_icon_instead_of_the_executable_icon() {
        let _apartment = Apartment::enter();
        let root =
            std::env::temp_dir().join(format!("echobird-desktop-icon-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let icon_path = root.join("registered.ico");
        let shortcut = root.join("application.lnk");
        let frames: Vec<_> = [16, 32, 64]
            .into_iter()
            .map(|size| {
                let image = image::RgbaImage::from_fn(size, size, |x, _| {
                    image::Rgba(if size < 64 {
                        [0, 255, 0, 255]
                    } else if x < 4 {
                        [200, 100, 50, 128]
                    } else if x / 4 % 2 == 0 {
                        [255, 0, 0, 255]
                    } else {
                        [0, 0, 255, 255]
                    })
                });
                IcoFrame::as_png(image.as_raw(), size, size, image::ExtendedColorType::Rgba8)
                    .unwrap()
            })
            .collect();
        IcoEncoder::new(std::fs::File::create(&icon_path).unwrap())
            .encode_images(&frames)
            .unwrap();
        let executable = std::env::current_exe()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        unsafe {
            let link: IShellLinkW =
                CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER).unwrap();
            link.SetPath(&HSTRING::from(&executable)).unwrap();
            link.SetIconLocation(&HSTRING::from(icon_path.to_string_lossy().as_ref()), 0)
                .unwrap();
            link.cast::<IPersistFile>()
                .unwrap()
                .Save(&HSTRING::from(shortcut.to_string_lossy().as_ref()), true)
                .unwrap();
        }
        assert!(shortcut_matches(&shortcut, &executable));
        assert!(shortcut_icon(&root, "missing.exe", 0).is_none());
        let result = shortcut_icon(&root, &executable, 0);
        std::fs::remove_file(&shortcut).unwrap();
        std::fs::remove_file(&icon_path).unwrap();
        std::fs::remove_dir(&root).unwrap();
        let image = result.unwrap().to_rgba8();
        assert_eq!(image.dimensions(), (64, 64));
        // Shell may center the icon or include its shortcut overlay. Check the
        // registered 64px frame's colors, independently of the shell's padding.
        assert!(
            image.pixels().any(|p| p.0 == [255, 0, 0, 255]),
            "missing high-resolution red frame"
        );
        assert!(
            image.pixels().any(|p| p.0 == [0, 0, 255, 255]),
            "missing high-resolution blue frame"
        );
        assert!(
            !image.pixels().any(|p| p.0 == [0, 255, 0, 255]),
            "shell selected a low-resolution frame"
        );
        assert!(
            image.pixels().any(|pixel| pixel
                .0
                .into_iter()
                .zip([200_u8, 100, 50, 128])
                .all(|(actual, expected)| actual.abs_diff(expected) <= 1)),
            "transparent edge colors changed"
        );
    }

    #[test]
    #[ignore = "read-only installed desktop application icon; requires explicitly selected HWND"]
    fn live_registered_application_icon() {
        use base64::Engine;
        let window = std::env::var("ECHOBIRD_ICON_TEST_WINDOW")
            .unwrap()
            .parse()
            .unwrap();
        let image = read(window).expect("system desktop icon must be available");
        eprintln!(
            "System desktop icon source: {}x{}",
            image.width(),
            image.height()
        );
        eprintln!(
            "Alpha edge samples: {:?}",
            image
                .to_rgba8()
                .pixels()
                .filter(|pixel| pixel[3] > 0 && pixel[3] < 255)
                .take(8)
                .collect::<Vec<_>>()
        );
        assert!(image.width() >= ICON_SIZE && image.height() >= ICON_SIZE);
        let encoded = super::super::encode_image(image).unwrap();
        let png = base64::engine::general_purpose::STANDARD
            .decode(encoded.strip_prefix("data:image/png;base64,").unwrap())
            .unwrap();
        let output = std::env::var("ECHOBIRD_ICON_TEST_OUTPUT").unwrap();
        std::fs::write(output, png).unwrap();
    }
}
