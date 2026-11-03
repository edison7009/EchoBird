use super::{confirm_write, native, Edit, InputTarget, MAX_FIELD_BYTES, UNAVAILABLE};
use tokio_util::sync::CancellationToken;
use windows::core::{AgileReference, BSTR};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoUninitialize, CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED,
};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, IUIAutomation2, IUIAutomationElement, IUIAutomationTextPattern,
    IUIAutomationValuePattern, TextPatternRangeEndpoint_End, TextPatternRangeEndpoint_Start,
    UIA_TextPatternId, UIA_ValuePatternId,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetAncestor, GetClassNameW, GetWindowLongPtrW, GetWindowThreadProcessId, IsWindow,
    IsWindowUnicode, SendMessageTimeoutW, ES_PASSWORD, ES_READONLY, GA_ROOT, GWL_STYLE,
    SMTO_ABORTIFHUNG, SMTO_BLOCK, WM_GETTEXT, WM_GETTEXTLENGTH,
};

struct Apartment;
impl Apartment {
    fn new() -> Option<Self> {
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED).ok().ok()? };
        Some(Self)
    }
}
impl Drop for Apartment {
    fn drop(&mut self) {
        unsafe { CoUninitialize() };
    }
}

enum Control {
    Edit {
        handle: u64,
        start: u32,
        end: u32,
        edit: Edit,
    },
    Automation {
        reference: AgileReference<IUIAutomationElement>,
        edit: Edit,
    },
    ForegroundOnly {
        reference: AgileReference<IUIAutomationElement>,
        input: InputTarget,
        original: String,
    },
}

pub(super) struct Target {
    window: u64,
    pid: u32,
    control: Control,
}

fn send(handle: u64, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<usize> {
    let mut result = 0;
    let delivered = unsafe {
        SendMessageTimeoutW(
            HWND(handle as *mut _),
            message,
            wparam,
            lparam,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            250,
            Some(&mut result),
        )
    };
    (delivered.0 != 0).then_some(result)
}

fn value(handle: u64) -> Option<String> {
    let length = send(handle, WM_GETTEXTLENGTH, WPARAM(0), LPARAM(0))?;
    if length > MAX_FIELD_BYTES / 2 {
        return None;
    }
    let mut text = vec![0_u16; length + 1];
    let copied = send(
        handle,
        WM_GETTEXT,
        WPARAM(text.len()),
        LPARAM(text.as_mut_ptr() as isize),
    )?;
    String::from_utf16(text.get(..copied)?).ok()
}

fn ambiguous_value(value: &str, name: &str, help: &str) -> bool {
    !value.is_empty() && (value == name || value == help)
}

fn web_editor(framework: &str, aria_role: &str, window_class: &str) -> bool {
    framework.eq_ignore_ascii_case("Chrome")
        || !aria_role.is_empty()
        || window_class.starts_with("Chrome_")
}

fn class_name(handle: u64) -> String {
    let mut class = [0_u16; 128];
    let length = unsafe { GetClassNameW(HWND(handle as *mut _), &mut class) }.max(0) as usize;
    String::from_utf16_lossy(&class[..length])
}

fn belongs_to_window(handle: HWND, window: u64) -> bool {
    !handle.0.is_null() && unsafe { GetAncestor(handle, GA_ROOT) }.0 as u64 == window
}

fn automation_in_window(
    automation: &IUIAutomation2,
    element: &IUIAutomationElement,
    window: u64,
) -> Option<()> {
    unsafe {
        let walker = automation.ControlViewWalker().ok()?;
        let mut node = element.clone();
        for _ in 0..32 {
            let handle = node.CurrentNativeWindowHandle().ok()?;
            if !handle.0.is_null() {
                return belongs_to_window(handle, window).then_some(());
            }
            node = walker.GetParentElement(&node).ok()?;
        }
    }
    None
}

pub(super) fn capture(input: Option<InputTarget>) -> Option<Target> {
    let input = input?;
    let mut pid = 0;
    unsafe { GetWindowThreadProcessId(HWND(input.window as *mut _), Some(&mut pid)) };
    if pid == 0 {
        return None;
    }
    if let Some(handle) = input.focus {
        if !belongs_to_window(HWND(handle as *mut _), input.window) {
            return None;
        }
        let class = class_name(handle);
        if class.eq_ignore_ascii_case("Edit") || class.to_ascii_lowercase().starts_with("richedit")
        {
            let style = unsafe { GetWindowLongPtrW(HWND(handle as *mut _), GWL_STYLE) };
            if style & (ES_PASSWORD | ES_READONLY) as isize != 0
                || !unsafe { IsWindowUnicode(HWND(handle as *mut _)) }.as_bool()
            {
                return None;
            }
            let mut start = 0_u32;
            let mut end = 0_u32;
            send(
                handle,
                0x00B0,
                WPARAM(&mut start as *mut u32 as usize),
                LPARAM(&mut end as *mut u32 as isize),
            )?;
            let original = value(handle)?;
            let edit = Edit::utf16(original, start as usize, end as usize)?;
            return Some(Target {
                window: input.window,
                pid,
                control: Control::Edit {
                    handle,
                    start,
                    end,
                    edit,
                },
            });
        }
    }
    let _apartment = Apartment::new()?;
    unsafe {
        let automation: IUIAutomation2 =
            CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER).ok()?;
        automation.SetAutoSetFocus(false).ok()?;
        automation.SetConnectionTimeout(200).ok()?;
        automation.SetTransactionTimeout(250).ok()?;
        let element = automation.GetFocusedElement().ok()?;
        automation_in_window(&automation, &element, input.window)?;
        if element.CurrentIsPassword().ok()?.as_bool() {
            return None;
        }
        let value = element
            .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            .ok()?;
        if value.CurrentIsReadOnly().ok()?.as_bool() {
            return None;
        }
        let original = value.CurrentValue().ok()?.to_string();
        if original.len() > MAX_FIELD_BYTES {
            return None;
        }
        let name = element.CurrentName().ok().map(|text| text.to_string());
        let help = element.CurrentHelpText().ok().map(|text| text.to_string());
        let framework = element
            .CurrentFrameworkId()
            .ok()
            .map(|text| text.to_string());
        let aria_role = element.CurrentAriaRole().ok().map(|text| text.to_string());
        let web = web_editor(
            framework.as_deref().unwrap_or_default(),
            aria_role.as_deref().unwrap_or_default(),
            &class_name(input.window),
        );
        let ambiguous = ambiguous_value(
            &original,
            name.as_deref().unwrap_or_default(),
            help.as_deref().unwrap_or_default(),
        );
        // Web values can include generated UI text even without a matching
        // accessible label. A whole-value rewrite would make that text real.
        if web || ambiguous {
            log::info!(
                "[VoiceInput] capture kind=foreground_only web_editor={} ambiguous_value={} value_chars={} name_chars={} help_chars={}",
                web,
                ambiguous,
                original.chars().count(),
                name.as_deref().unwrap_or_default().chars().count(),
                help.as_deref().unwrap_or_default().chars().count()
            );
            if !input.matches(native::input_target()) {
                return None;
            }
            return Some(Target {
                window: input.window,
                pid,
                control: Control::ForegroundOnly {
                    reference: AgileReference::new(&element).ok()?,
                    input,
                    original,
                },
            });
        }
        let mut node = element.clone();
        let walker = automation.ControlViewWalker().ok()?;
        for _ in 0..5 {
            if let Ok(pattern) =
                node.GetCurrentPatternAs::<IUIAutomationTextPattern>(UIA_TextPatternId)
            {
                let edit = (|| {
                    let selections = pattern.GetSelection().ok()?;
                    if selections.Length().ok()? != 1 {
                        return None;
                    }
                    let selected = selections.GetElement(0).ok()?;
                    let full = if automation.CompareElements(&node, &element).ok()?.as_bool() {
                        pattern.DocumentRange().ok()?
                    } else {
                        pattern.RangeFromChild(&element).ok()?
                    };
                    let before = full.Clone().ok()?;
                    before
                        .MoveEndpointByRange(
                            TextPatternRangeEndpoint_End,
                            &selected,
                            TextPatternRangeEndpoint_Start,
                        )
                        .ok()?;
                    let after = full.Clone().ok()?;
                    after
                        .MoveEndpointByRange(
                            TextPatternRangeEndpoint_Start,
                            &selected,
                            TextPatternRangeEndpoint_End,
                        )
                        .ok()?;
                    Edit::surrounding(
                        original.clone(),
                        &before.GetText(MAX_FIELD_BYTES as i32).ok()?.to_string(),
                        &selected.GetText(MAX_FIELD_BYTES as i32).ok()?.to_string(),
                        &after.GetText(MAX_FIELD_BYTES as i32).ok()?.to_string(),
                    )
                })();
                if let Some(edit) = edit {
                    if !input.matches(native::input_target()) {
                        return None;
                    }
                    return Some(Target {
                        window: input.window,
                        pid,
                        control: Control::Automation {
                            reference: AgileReference::new(&element).ok()?,
                            edit,
                        },
                    });
                }
            }
            node = walker.GetParentElement(&node).ok()?;
        }
    }
    None
}

impl Target {
    pub(super) fn insert(self, text: &str, cancel: &CancellationToken) -> Result<(), String> {
        if cancel.is_cancelled() {
            return Err("voice.error.cancelled".into());
        }
        let mut pid = 0;
        unsafe { GetWindowThreadProcessId(HWND(self.window as *mut _), Some(&mut pid)) };
        if pid != self.pid || !unsafe { IsWindow(Some(HWND(self.window as *mut _))) }.as_bool() {
            return Err(UNAVAILABLE.into());
        }
        match self.control {
            Control::Edit {
                handle,
                start,
                end,
                edit,
            } => {
                if !belongs_to_window(HWND(handle as *mut _), self.window) {
                    return Err(UNAVAILABLE.into());
                }
                if unsafe { GetWindowLongPtrW(HWND(handle as *mut _), GWL_STYLE) }
                    & ES_READONLY as isize
                    != 0
                {
                    return Err(UNAVAILABLE.into());
                }
                let current = value(handle).ok_or(UNAVAILABLE)?;
                let text = text
                    .replace("\r\n", "\n")
                    .replace('\r', "\n")
                    .replace('\n', "\r\n");
                let expected = edit.replace(&current, &text)?;
                if cancel.is_cancelled() {
                    return Err("voice.error.cancelled".into());
                }
                send(handle, 0x00B1, WPARAM(start as usize), LPARAM(end as isize))
                    .ok_or(UNAVAILABLE)?;
                let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
                let acknowledged =
                    send(handle, 0x00C2, WPARAM(1), LPARAM(wide.as_ptr() as isize)).is_some();
                let verified = confirm_write(&expected, false, || value(handle));
                log::info!(
                    "[VoiceInput] background write kind=native_edit acknowledged={} verified={}",
                    acknowledged,
                    verified.is_ok()
                );
                verified
            }
            Control::Automation { reference, edit } => {
                let _apartment = Apartment::new().ok_or(UNAVAILABLE)?;
                unsafe {
                    let element = reference.resolve().map_err(|_| UNAVAILABLE)?;
                    let value = element
                        .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                        .map_err(|_| UNAVAILABLE)?;
                    if value
                        .CurrentIsReadOnly()
                        .map_err(|_| UNAVAILABLE)?
                        .as_bool()
                    {
                        return Err(UNAVAILABLE.into());
                    }
                    let current = value.CurrentValue().map_err(|_| UNAVAILABLE)?.to_string();
                    let output = edit.replace(&current, text)?;
                    if cancel.is_cancelled() {
                        return Err("voice.error.cancelled".into());
                    }
                    let acknowledged = value.SetValue(&BSTR::from(output.as_str())).is_ok();
                    let confirmed = confirm_write(&output, acknowledged, || {
                        value.CurrentValue().ok().map(|text| text.to_string())
                    });
                    log::info!(
                        "[VoiceInput] background write kind=automation acknowledged={} confirmed={}",
                        acknowledged,
                        confirmed.is_ok()
                    );
                    confirmed
                }
            }
            Control::ForegroundOnly {
                reference,
                input,
                original,
            } => {
                if !input.matches(native::input_target()) {
                    log::info!("[VoiceInput] foreground input skipped reason=focus_moved");
                    return Err(UNAVAILABLE.into());
                }
                let _apartment = Apartment::new().ok_or(UNAVAILABLE)?;
                let element = reference.resolve().map_err(|_| UNAVAILABLE)?;
                if !unsafe { element.CurrentHasKeyboardFocus() }
                    .map_err(|_| UNAVAILABLE)?
                    .as_bool()
                {
                    log::info!("[VoiceInput] foreground input skipped reason=control_unfocused");
                    return Err(UNAVAILABLE.into());
                }
                // Pasting lands at the caret of this same focused control, so edits
                // made while speaking (drafts, auto-formatting) must not block it. A
                // value that no longer contains the captured text means the editor now
                // shows other content, such as another conversation in the same page.
                let current = unsafe {
                    element.GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
                }
                .and_then(|value| unsafe { value.CurrentValue() })
                .map_err(|_| UNAVAILABLE)?
                .to_string();
                if !kept_original(&original, &current) {
                    log::info!("[VoiceInput] foreground input skipped reason=content_replaced");
                    return Err(UNAVAILABLE.into());
                }
                let changed = current != original;
                log::info!("[VoiceInput] foreground value_changed={changed}");
                log::info!("[VoiceInput] input route=foreground_automation");
                native::insert(text, Some(input), cancel)
            }
        }
    }
}

/// Whether an editor still holds the text captured when dictation began.
fn kept_original(original: &str, current: &str) -> bool {
    let normalize = |text: &str| text.replace("\r\n", "\n").replace('\r', "\n");
    let original = normalize(original);
    let original = original.trim();
    original.is_empty() || normalize(current).contains(original)
}

#[cfg(test)]
#[path = "windows_tests.rs"]
mod tests;
