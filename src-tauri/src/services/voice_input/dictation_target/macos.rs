use super::{native, Edit, InputTarget, MAX_FIELD_BYTES, UNAVAILABLE};
use core_foundation::base::{CFEqual, CFGetTypeID, CFRange, CFRelease, CFTypeRef, TCFType};
use core_foundation::boolean::CFBoolean;
use core_foundation::dictionary::{CFDictionary, CFDictionaryRef};
use core_foundation::string::{CFString, CFStringGetTypeID, CFStringRef};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_util::sync::CancellationToken;

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXIsProcessTrusted() -> bool;
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> bool;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
    fn AXUIElementCreateApplication(pid: i32) -> CFTypeRef;
    fn AXUIElementCopyAttributeValue(
        element: CFTypeRef,
        name: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXUIElementSetAttributeValue(element: CFTypeRef, name: CFStringRef, value: CFTypeRef)
        -> i32;
    fn AXUIElementIsAttributeSettable(
        element: CFTypeRef,
        name: CFStringRef,
        settable: *mut bool,
    ) -> i32;
    fn AXUIElementSetMessagingTimeout(element: CFTypeRef, timeout: f32) -> i32;
    fn AXValueGetValue(value: CFTypeRef, kind: i32, output: *mut std::ffi::c_void) -> bool;
}

#[link(name = "Carbon", kind = "framework")]
extern "C" {
    fn IsSecureEventInputEnabled() -> u8;
}

/// Whether typing now could reach a password field: macOS turns on secure input
/// while any one has focus, including in apps whose fields AX cannot see.
pub(super) fn secure_input(input: InputTarget) -> bool {
    if unsafe { IsSecureEventInputEnabled() } != 0 {
        return true;
    }
    let app = unsafe { AXUIElementCreateApplication(input.window as i32) };
    if app.is_null() {
        return false;
    }
    let app = Owned(app);
    unsafe { AXUIElementSetMessagingTimeout(app.0, 0.25) };
    attribute(&app, "AXFocusedUIElement")
        .and_then(|element| attribute(&element, "AXSubrole"))
        .and_then(|role| string(&role))
        .as_deref()
        == Some("AXSecureTextField")
}

pub(super) fn trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Shows the system's request to allow EchoBird under Accessibility, at most once per
/// launch, so a dictation that could not be written tells the user what to turn on.
pub(super) fn request_trust() {
    static ASKED: AtomicBool = AtomicBool::new(false);
    ask_once(&ASKED, || {
        // SAFETY: a constant CFString exported by ApplicationServices.
        let prompt = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
        let options = CFDictionary::from_CFType_pairs(&[(prompt, CFBoolean::true_value())]);
        unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) };
    });
}

fn ask_once(asked: &AtomicBool, ask: impl FnOnce()) {
    if !asked.swap(true, Ordering::AcqRel) {
        ask();
    }
}

struct Owned(CFTypeRef);
// Retained AX/CF value objects use the thread-safe accessibility messaging API;
// no AppKit UI object or main-thread-only operation crosses into the worker.
unsafe impl Send for Owned {}
impl Drop for Owned {
    fn drop(&mut self) {
        unsafe { CFRelease(self.0) };
    }
}

fn attribute(element: &Owned, name: &str) -> Option<Owned> {
    let name = CFString::new(name);
    let mut value = std::ptr::null();
    if unsafe { AXUIElementCopyAttributeValue(element.0, name.as_concrete_TypeRef(), &mut value) }
        == 0
        && !value.is_null()
    {
        Some(Owned(value))
    } else {
        None
    }
}

fn string(value: &Owned) -> Option<String> {
    (unsafe { CFGetTypeID(value.0) == CFStringGetTypeID() })
        .then(|| unsafe { CFString::wrap_under_get_rule(value.0.cast()).to_string() })
}

fn writable(element: &Owned, name: &str) -> bool {
    let mut settable = false;
    let name = CFString::new(name);
    unsafe {
        AXUIElementIsAttributeSettable(element.0, name.as_concrete_TypeRef(), &mut settable) == 0
            && settable
    }
}

fn set(element: &Owned, name: &str, value: CFTypeRef) -> Result<(), String> {
    let name = CFString::new(name);
    if unsafe { AXUIElementSetAttributeValue(element.0, name.as_concrete_TypeRef(), value) } == 0 {
        Ok(())
    } else {
        Err(UNAVAILABLE.into())
    }
}

pub(super) struct Target {
    input: InputTarget,
    app: Owned,
    element: Owned,
    range: Option<Owned>,
    edit: Option<Edit>,
    selected_text: bool,
    whole_value: bool,
}

pub(super) fn capture(input: Option<InputTarget>) -> Option<Target> {
    let input = input?;
    if !unsafe { AXIsProcessTrusted() } {
        return None;
    }
    let app = unsafe { AXUIElementCreateApplication(input.window as i32) };
    if app.is_null() {
        return None;
    }
    let app = Owned(app);
    unsafe { AXUIElementSetMessagingTimeout(app.0, 0.25) };
    let element = attribute(&app, "AXFocusedUIElement")?;
    unsafe { AXUIElementSetMessagingTimeout(element.0, 0.25) };
    if attribute(&element, "AXSubrole")
        .and_then(|role| string(&role))
        .as_deref()
        == Some("AXSecureTextField")
    {
        return None;
    }
    let range = attribute(&element, "AXSelectedTextRange");
    let edit = (|| {
        let original = string(&attribute(&element, "AXValue")?)?;
        if original.len() > MAX_FIELD_BYTES {
            return None;
        }
        let mut location = CFRange {
            location: 0,
            length: 0,
        };
        if !unsafe { AXValueGetValue(range.as_ref()?.0, 4, (&mut location as *mut CFRange).cast()) }
        {
            return None;
        }
        let start = usize::try_from(location.location).ok()?;
        let end = usize::try_from(location.location.checked_add(location.length)?).ok()?;
        Edit::utf16(original, start, end)
    })();
    let selected_text =
        writable(&element, "AXSelectedTextRange") && writable(&element, "AXSelectedText");
    let whole_value = writable(&element, "AXValue");
    Some(Target {
        input,
        app,
        element,
        range,
        edit,
        selected_text,
        whole_value,
    })
}

impl Target {
    pub(super) fn insert(self, text: &str, cancel: &CancellationToken) -> Result<(), String> {
        if cancel.is_cancelled() {
            return Err("voice.error.cancelled".into());
        }
        if let Some(edit) = &self.edit {
            let current = attribute(&self.element, "AXValue")
                .and_then(|value| string(&value))
                .ok_or(UNAVAILABLE)?;
            let replacement = edit.replace(&current, text)?;
            if cancel.is_cancelled() {
                return Err("voice.error.cancelled".into());
            }
            if self.selected_text {
                set(
                    &self.element,
                    "AXSelectedTextRange",
                    self.range.as_ref().ok_or(UNAVAILABLE)?.0,
                )?;
                let text = CFString::new(text);
                return set(&self.element, "AXSelectedText", text.as_CFTypeRef());
            }
            if self.whole_value {
                let value = CFString::new(&replacement);
                return set(&self.element, "AXValue", value.as_CFTypeRef());
            }
        }
        // Keep the existing keyboard path only when the exact original field
        // and its selection still own focus. Never activate an application.
        let focused = attribute(&self.app, "AXFocusedUIElement").ok_or(UNAVAILABLE)?;
        let range = attribute(&self.element, "AXSelectedTextRange").ok_or(UNAVAILABLE)?;
        if native::foreground() != Some(self.input.window)
            || unsafe { CFEqual(focused.0, self.element.0) } == 0
            || unsafe { CFEqual(range.0, self.range.as_ref().ok_or(UNAVAILABLE)?.0) } == 0
        {
            return Err(UNAVAILABLE.into());
        }
        native::insert(text, Some(self.input), cancel)
    }
}

#[cfg(test)]
mod trust_tests {
    use super::*;

    #[test]
    fn the_accessibility_request_is_shown_once_per_launch() {
        let asked = AtomicBool::new(false);
        let mut shown = 0;
        for _ in 0..3 {
            ask_once(&asked, || shown += 1);
        }
        assert_eq!(shown, 1);
    }
}
