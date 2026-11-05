use super::config::Binding;
#[cfg(not(windows))]
use enigo::{Enigo, Keyboard, Settings};

pub fn wayland() -> bool {
    cfg!(target_os = "linux") && std::env::var_os("WAYLAND_DISPLAY").is_some()
}

pub fn foreground() -> Option<u64> {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::WindowsAndMessaging::{
            GetForegroundWindow, GetWindowThreadProcessId,
        };
        let window = GetForegroundWindow();
        let mut pid = 0;
        GetWindowThreadProcessId(window, Some(&mut pid));
        (!window.0.is_null() && pid != std::process::id()).then_some(window.0 as u64)
    }
    #[cfg(target_os = "macos")]
    {
        let pid = objc2_app_kit::NSWorkspace::sharedWorkspace()
            .frontmostApplication()?
            .processIdentifier() as u64;
        (pid != std::process::id() as u64).then_some(pid)
    }
    #[cfg(target_os = "linux")]
    {
        use x11rb::connection::Connection;
        use x11rb::protocol::xproto::{AtomEnum, ConnectionExt};
        if wayland() {
            return None;
        }
        let (connection, screen) = x11rb::connect(None).ok()?;
        let atom = connection
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")
            .ok()?
            .reply()
            .ok()?
            .atom;
        let property = connection
            .get_property(
                false,
                connection.setup().roots[screen].root,
                atom,
                AtomEnum::WINDOW,
                0,
                1,
            )
            .ok()?
            .reply()
            .ok()?;
        let window = property.value32()?.next().map(u64::from);
        window.filter(|id| *id != 0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InputTarget {
    pub window: u64,
    pub focus: Option<u64>,
}

impl InputTarget {
    pub fn matches(self, current: Option<Self>) -> bool {
        current.is_some_and(|current| {
            self.window == current.window
                && self.focus.is_none_or(|focus| current.focus == Some(focus))
        })
    }
}

pub fn input_target() -> Option<InputTarget> {
    let window = foreground()?;
    #[cfg(not(windows))]
    let focus = None;
    #[cfg(windows)]
    let focus = unsafe {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            GetGUIThreadInfo, GetWindowThreadProcessId, GUITHREADINFO,
        };
        let thread = GetWindowThreadProcessId(HWND(window as *mut _), None);
        let mut info = GUITHREADINFO {
            cbSize: std::mem::size_of::<GUITHREADINFO>() as u32,
            ..Default::default()
        };
        (thread != 0 && GetGUIThreadInfo(thread, &mut info).is_ok() && !info.hwndFocus.0.is_null())
            .then_some(info.hwndFocus.0 as u64)
    };
    Some(InputTarget { window, focus })
}

#[cfg(any(windows, test))]
pub(super) fn alt_menu_key(binding: &Binding) -> Option<u16> {
    if !binding.modifiers.is_empty() {
        return None;
    }
    match binding.code.as_str() {
        "AltLeft" => Some(0xA4),
        "AltRight" => Some(0xA5),
        _ => None,
    }
}

pub fn insert(
    text: &str,
    target: Option<InputTarget>,
    cancel: &tokio_util::sync::CancellationToken,
) -> Result<(), String> {
    if cancel.is_cancelled() {
        return Err("voice.error.cancelled".into());
    }
    let current = input_target();
    if let Some(target) = target {
        log::info!(
            "[VoiceInput] input target window_same={} focus_known={} focus_same={}",
            current.is_some_and(|current| current.window == target.window),
            target.focus.is_some(),
            current.is_some_and(|current| current.focus == target.focus)
        );
        if !target.matches(current) {
            return Err("voice.error.focus".into());
        }
    }
    #[cfg(windows)]
    {
        super::paste_windows::insert(text, target, cancel)
    }
    #[cfg(not(windows))]
    {
        let mut keyboard = Enigo::new(&Settings::default()).map_err(|_| "voice.error.insert")?;
        keyboard.text(text).map_err(|_| "voice.error.insert".into())
    }
}

pub fn modifier(binding: &Binding) -> bool {
    matches!(
        binding.code.as_str(),
        "AltLeft"
            | "AltRight"
            | "ControlLeft"
            | "ControlRight"
            | "ShiftLeft"
            | "ShiftRight"
            | "MetaLeft"
            | "MetaRight"
    )
}

pub struct ModifierMonitor {
    code: u32,
    #[cfg(target_os = "linux")]
    connection: x11rb::rust_connection::RustConnection,
}

impl ModifierMonitor {
    pub fn escape() -> Option<Self> {
        if wayland() {
            return None;
        }
        #[cfg(target_os = "macos")]
        if !unsafe { CGPreflightListenEventAccess() } {
            return None;
        }
        Self::new(&Binding {
            code: "Escape".into(),
            modifiers: Vec::new(),
        })
        .ok()
    }

    pub fn new(binding: &Binding) -> Result<Self, String> {
        if !binding.modifiers.is_empty() {
            return Err("voice.error.shortcut".into());
        }
        #[cfg(windows)]
        let code = match binding.code.as_str() {
            "Escape" => 0x1B,
            "AltLeft" => 0xA4,
            "AltRight" => 0xA5,
            "ControlLeft" => 0xA2,
            "ControlRight" => 0xA3,
            "ShiftLeft" => 0xA0,
            "ShiftRight" => 0xA1,
            "MetaLeft" => 0x5B,
            "MetaRight" => 0x5C,
            _ => return Err("voice.error.shortcut".into()),
        };
        #[cfg(target_os = "macos")]
        let code = {
            if !unsafe { CGPreflightListenEventAccess() } {
                unsafe {
                    CGRequestListenEventAccess();
                }
                return Err("voice.error.shortcutPermission".into());
            }
            match binding.code.as_str() {
                "Escape" => 53,
                "AltLeft" => 58,
                "AltRight" => 61,
                "ControlLeft" => 59,
                "ControlRight" => 62,
                "ShiftLeft" => 56,
                "ShiftRight" => 60,
                "MetaLeft" => 55,
                "MetaRight" => 54,
                _ => return Err("voice.error.shortcut".into()),
            }
        };
        #[cfg(target_os = "linux")]
        {
            use x11rb::connection::Connection;
            use x11rb::protocol::xproto::ConnectionExt;
            let keysym = match binding.code.as_str() {
                "Escape" => 0xff1b,
                "AltLeft" => 0xffe9,
                "AltRight" => 0xffea,
                "ControlLeft" => 0xffe3,
                "ControlRight" => 0xffe4,
                "ShiftLeft" => 0xffe1,
                "ShiftRight" => 0xffe2,
                "MetaLeft" => 0xffeb,
                "MetaRight" => 0xffec,
                _ => return Err("voice.error.shortcut".into()),
            };
            let (connection, _) = x11rb::connect(None).map_err(|_| "voice.error.shortcut")?;
            let setup = connection.setup();
            let mapping = connection
                .get_keyboard_mapping(setup.min_keycode, setup.max_keycode - setup.min_keycode + 1)
                .map_err(|_| "voice.error.shortcut")?
                .reply()
                .map_err(|_| "voice.error.shortcut")?;
            let index = mapping
                .keysyms
                .chunks(mapping.keysyms_per_keycode as usize)
                .position(|keys| keys.contains(&keysym))
                .ok_or("voice.error.shortcut")?;
            let code = setup.min_keycode as u32 + index as u32;
            Ok(Self { code, connection })
        }
        #[cfg(not(target_os = "linux"))]
        Ok(Self { code })
    }

    pub fn down(&self) -> bool {
        #[cfg(windows)]
        {
            unsafe {
                windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(self.code as i32) < 0
            }
        }
        #[cfg(target_os = "macos")]
        {
            unsafe { CGEventSourceKeyState(1, self.code as u16) }
        }
        #[cfg(target_os = "linux")]
        {
            use x11rb::protocol::xproto::ConnectionExt;
            self.connection
                .query_keymap()
                .ok()
                .and_then(|cookie| cookie.reply().ok())
                .is_some_and(|reply| {
                    reply.keys[self.code as usize / 8] & (1 << (self.code % 8)) != 0
                })
        }
    }
}

#[cfg(target_os = "macos")]
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceButtonState(state: i32, button: u32) -> bool;
    fn CGEventSourceKeyState(state: i32, key: u16) -> bool;
    fn CGPreflightListenEventAccess() -> bool;
    fn CGRequestListenEventAccess() -> bool;
}

#[cfg(any(windows, target_os = "macos"))]
pub(crate) fn left_mouse_down() -> bool {
    #[cfg(windows)]
    unsafe {
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON};
        GetAsyncKeyState(VK_LBUTTON.0 as i32) < 0
    }
    #[cfg(target_os = "macos")]
    unsafe {
        CGEventSourceButtonState(1, 0)
    }
}

#[cfg(any(windows, target_os = "macos"))]
pub(super) fn selection_keys_down(copying: bool) -> bool {
    #[cfg(windows)]
    {
        (8..255)
            .filter(|key| {
                !([16, 160, 161].contains(key) || copying && [17, 67, 162, 163].contains(key))
            })
            .any(|key| unsafe {
                windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState(key) < 0
            })
    }
    #[cfg(target_os = "macos")]
    {
        let _ = copying;
        (0..128)
            .filter(|key| ![56, 60].contains(key))
            .any(|key| unsafe { CGEventSourceKeyState(1, key) })
    }
}
