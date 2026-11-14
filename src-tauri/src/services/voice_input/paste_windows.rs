use super::native::{self, InputTarget};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
use windows::core::{w, Error, Result, PCWSTR};
use windows::Win32::Foundation::{
    GetLastError, GlobalFree, SetLastError, HANDLE, HGLOBAL, HWND, WIN32_ERROR,
};
use windows::Win32::Graphics::Gdi::{
    CopyEnhMetaFileW, DeleteEnhMetaFile, DeleteMetaFile, DeleteObject, HENHMETAFILE, HGDIOBJ,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, EnumClipboardFormats, GetClipboardData, GetClipboardOwner,
    GetClipboardSequenceNumber, OpenClipboard, RegisterClipboardFormatW, SetClipboardData,
    METAFILEPICT,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{
    OleDuplicateData, CF_BITMAP, CF_DSPBITMAP, CF_DSPENHMETAFILE, CF_DSPMETAFILEPICT,
    CF_ENHMETAFILE, CF_METAFILEPICT, CF_PALETTE, CF_UNICODETEXT, CLIPBOARD_FORMAT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYEVENTF_KEYUP,
    VIRTUAL_KEY, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE,
};

static INSERTING: Mutex<()> = Mutex::new(());

mod selection_copy;
pub(super) use selection_copy::copy_selection;

struct Owner(HWND);

impl Owner {
    fn new() -> Result<Self> {
        unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                w!("STATIC"),
                w!(""),
                WINDOW_STYLE::default(),
                0,
                0,
                0,
                0,
                Some(HWND_MESSAGE),
                None,
                None,
                None,
            )
            .map(Self)
        }
    }
}

impl Drop for Owner {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

struct Open;

impl Open {
    fn new(owner: HWND) -> Result<Self> {
        for _ in 0..20 {
            if unsafe { OpenClipboard(Some(owner)) }.is_ok() {
                return Ok(Self);
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        unsafe {
            OpenClipboard(Some(owner))?;
        }
        Ok(Self)
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

// Own a duplicate, never the clipboard's borrowed handle. Transferring it back
// to Windows clears the handle so Drop cannot free the clipboard's data.
struct Data {
    format: u32,
    handle: HANDLE,
}

impl Data {
    fn duplicate(format: u32) -> Result<Self> {
        unsafe {
            let source = GetClipboardData(format)?;
            let kind = match CLIPBOARD_FORMAT(format as u16) {
                CF_DSPBITMAP => CF_BITMAP,
                CF_DSPMETAFILEPICT => CF_METAFILEPICT,
                CF_DSPENHMETAFILE => CF_ENHMETAFILE,
                other => other,
            };
            let handle = match kind {
                CF_ENHMETAFILE => {
                    HANDLE(CopyEnhMetaFileW(HENHMETAFILE(source.0), PCWSTR::null()).0)
                }
                _ => OleDuplicateData(source, kind, GMEM_MOVEABLE),
            };
            if handle.is_invalid() {
                return Err(Error::from_thread());
            }
            Ok(Self { format, handle })
        }
    }

    fn bytes(format: u32, bytes: &[u8]) -> Result<Self> {
        unsafe {
            let handle = GlobalAlloc(GMEM_MOVEABLE, bytes.len())?;
            let data = Self {
                format,
                handle: HANDLE(handle.0),
            };
            let buffer = GlobalLock(handle).cast::<u8>();
            if buffer.is_null() {
                return Err(Error::from_thread());
            }
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), buffer, bytes.len());
            let _ = GlobalUnlock(handle);
            Ok(data)
        }
    }

    fn put(&mut self) -> Result<()> {
        unsafe {
            SetClipboardData(self.format, Some(self.handle))?;
        }
        self.handle = HANDLE::default();
        Ok(())
    }
}

impl Drop for Data {
    fn drop(&mut self) {
        if self.handle.is_invalid() {
            return;
        }
        unsafe {
            match CLIPBOARD_FORMAT(self.format as u16) {
                CF_BITMAP | CF_PALETTE | CF_DSPBITMAP => {
                    let _ = DeleteObject(HGDIOBJ(self.handle.0));
                }
                CF_ENHMETAFILE | CF_DSPENHMETAFILE => {
                    let _ = DeleteEnhMetaFile(Some(HENHMETAFILE(self.handle.0)));
                }
                CF_METAFILEPICT | CF_DSPMETAFILEPICT => {
                    let global = HGLOBAL(self.handle.0);
                    let picture = GlobalLock(global).cast::<METAFILEPICT>();
                    if !picture.is_null() {
                        let _ = DeleteMetaFile((*picture).hMF);
                        let _ = GlobalUnlock(global);
                    }
                    let _ = GlobalFree(Some(global));
                }
                _ => {
                    let _ = GlobalFree(Some(HGLOBAL(self.handle.0)));
                }
            }
        }
    }
}

struct Clipboard {
    owner: Owner,
    saved: Vec<Data>,
    sequence: u32,
}

impl Clipboard {
    fn owned_sequence(owner: HWND) -> Result<u32> {
        let _opened = Open::new(owner)?;
        if unsafe { GetClipboardOwner() }? != owner {
            return Err(Error::from_thread());
        }
        Ok(unsafe { GetClipboardSequenceNumber() })
    }

    fn replace(text: &[u8]) -> Result<Self> {
        let owner = Owner::new()?;
        let mut replacement = Data::bytes(u32::from(CF_UNICODETEXT.0), text)?;
        let excluded =
            unsafe { RegisterClipboardFormatW(w!("ExcludeClipboardContentFromMonitorProcessing")) };
        if excluded == 0 {
            return Err(Error::from_thread());
        }
        let mut excluded = Data::bytes(excluded, &0_u32.to_le_bytes())?;
        let opened = Open::new(owner.0)?;
        let mut saved = Vec::new();
        let mut format = 0;
        loop {
            unsafe {
                SetLastError(WIN32_ERROR(0));
            }
            format = unsafe { EnumClipboardFormats(format) };
            if format == 0 {
                if unsafe { GetLastError() }.0 != 0 {
                    return Err(Error::from_thread());
                }
                break;
            }
            saved.push(Data::duplicate(format)?);
        }
        unsafe {
            EmptyClipboard()?;
        }
        if let Err(error) = excluded.put().and_then(|_| replacement.put()) {
            let _ = unsafe { EmptyClipboard() };
            for data in &mut saved {
                let _ = data.put();
            }
            return Err(error);
        }
        let sequence = unsafe { GetClipboardSequenceNumber() };
        drop(opened);
        let mut clipboard = Self {
            owner,
            saved,
            sequence,
        };
        // CloseClipboard can synthesize compatible text formats and advance
        // the sequence. Reopen and verify our unique owner before accepting it;
        // reading a sequence outside the lock could adopt somebody else's copy.
        clipboard.sequence = Self::owned_sequence(clipboard.owner.0)?;
        Ok(clipboard)
    }

    fn restore(&mut self) -> Result<Option<u32>> {
        // Check while open: another copy must win, including one made while we
        // were waiting for the clipboard lock. Never restore over newer content.
        if self.sequence == 0 {
            return Ok(None);
        }
        let opened = Open::new(self.owner.0)?;
        let sequence = std::mem::take(&mut self.sequence);
        if unsafe { GetClipboardSequenceNumber() } != sequence {
            return Ok(None);
        }
        self.restore_contents()?;
        drop(opened);
        Self::owned_sequence(self.owner.0).map(Some)
    }

    // Also used while an explicit copy's clipboard lock is still held, so a
    // synthesized format cannot change the generation between read and restore.
    fn restore_contents(&mut self) -> Result<()> {
        unsafe {
            EmptyClipboard()?;
        }
        let mut error = None;
        for data in &mut self.saved {
            if let Err(failed) = data.put() {
                error = Some(failed);
            }
        }
        if let Some(error) = error {
            return Err(error);
        }
        Ok(())
    }
}

impl Drop for Clipboard {
    fn drop(&mut self) {
        if self.restore().is_err() {
            log::warn!("[VoiceInput] original clipboard could not be fully restored");
        }
    }
}

fn text_data(text: &str) -> std::result::Result<Vec<u8>, String> {
    if text.contains('\0') {
        return Err("voice.error.insert".into());
    }
    Ok(text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .replace('\n', "\r\n")
        .encode_utf16()
        .chain(std::iter::once(0))
        .flat_map(u16::to_le_bytes)
        .collect())
}

fn key_inputs(key: VIRTUAL_KEY) -> [INPUT; 4] {
    [
        (VK_CONTROL, false),
        (key, false),
        (key, true),
        (VK_CONTROL, true),
    ]
    .map(|(key, released)| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: key,
                dwFlags: if released {
                    KEYEVENTF_KEYUP
                } else {
                    Default::default()
                },
                ..Default::default()
            },
        },
    })
}

fn paste_inputs() -> [INPUT; 4] {
    key_inputs(VIRTUAL_KEY(0x56))
}

fn dispatch(mut send: impl FnMut(&[INPUT]) -> u32) -> std::result::Result<(), String> {
    let inputs = paste_inputs();
    if send(&inputs) == inputs.len() as u32 {
        return Ok(());
    }
    // Release only; never repeat a possibly delivered paste or fall back to
    // typing, which could duplicate a partially accepted result.
    let _ = send(&inputs[2..]);
    Err("voice.error.insert".into())
}

pub(super) fn insert(
    text: &str,
    target: Option<InputTarget>,
    cancel: &CancellationToken,
) -> std::result::Result<(), String> {
    insert_with_confirmation(text, target, cancel, &mut || true, None)
}

pub(super) fn insert_with_confirmation(
    text: &str,
    target: Option<InputTarget>,
    cancel: &CancellationToken,
    validate: &mut dyn FnMut() -> bool,
    mut confirmed: Option<&mut dyn FnMut() -> bool>,
) -> std::result::Result<(), String> {
    if text.is_empty() {
        return Ok(());
    }
    if cancel.is_cancelled() {
        return Err("voice.error.cancelled".into());
    }
    let _inserting = INSERTING.try_lock().map_err(|_| "voice.error.busy")?;
    let data = text_data(text)?;
    let started = Instant::now();
    while modifiers_down() {
        if cancel.is_cancelled() {
            return Err("voice.error.cancelled".into());
        }
        if started.elapsed() >= Duration::from_secs(1) {
            return Err("voice.error.insert".into());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let _clipboard = Clipboard::replace(&data).map_err(|_| "voice.error.insert")?;
    if cancel.is_cancelled() {
        return Err("voice.error.cancelled".into());
    }
    if modifiers_down() {
        return Err("voice.error.insert".into());
    }
    if !validate() || target.is_some_and(|target| !target.matches(native::input_target())) {
        return Err("voice.error.focus".into());
    }
    let result =
        dispatch(|inputs| unsafe { SendInput(inputs, std::mem::size_of::<INPUT>() as i32) });
    // Keep the data available while the target processes the single paste.
    // Runtime remains busy until restoration, preventing a second transaction.
    std::thread::sleep(Duration::from_millis(500));
    result?;
    if let Some(confirmed) = &mut confirmed {
        return super::dictation_target::confirm_paste(cancel, *confirmed);
    }
    log::info!("[VoiceInput] paste delivery=dispatched");
    Ok(())
}

fn modifiers_down() -> bool {
    [VK_CONTROL, VK_SHIFT, VK_MENU, VK_LWIN, VK_RWIN]
        .into_iter()
        .any(|key| unsafe { GetAsyncKeyState(i32::from(key.0)) } < 0)
}

#[cfg(test)]
mod tests;
