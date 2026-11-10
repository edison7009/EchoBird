use super::native::{self, InputTarget};
use std::ops::Range;
use tokio_util::sync::CancellationToken;

#[cfg(windows)]
#[path = "dictation_target/windows.rs"]
mod platform;
#[cfg(target_os = "macos")]
#[path = "dictation_target/macos.rs"]
mod platform;
#[cfg(target_os = "linux")]
#[path = "dictation_target/linux.rs"]
mod platform;

const UNAVAILABLE: &str = "voice.error.target";
#[cfg(target_os = "macos")]
const UNTRUSTED: &str = "voice.error.accessibility";
const MAX_FIELD_BYTES: usize = 1_048_576;

pub(super) struct Target {
    foreground: Option<InputTarget>,
    background: Option<platform::Target>,
}

pub(super) fn capture(foreground: Option<InputTarget>) -> Option<Target> {
    let background = platform::capture(foreground);
    log::info!(
        "[VoiceInput] input snapshot foreground_known={} control_captured={}",
        foreground.is_some(),
        background.is_some()
    );
    (foreground.is_some() || background.is_some()).then_some(Target {
        foreground,
        background,
    })
}

pub(super) fn insert(
    text: &str,
    target: Option<Target>,
    cancel: &CancellationToken,
) -> Result<(), String> {
    if cancel.is_cancelled() {
        return Err("voice.error.cancelled".into());
    }
    if text.contains('\0') {
        return Err("voice.error.insert".into());
    }
    let target = target.ok_or(UNAVAILABLE)?;
    // Without Accessibility access macOS hides every other app's fields and keys.
    #[cfg(target_os = "macos")]
    if !platform::trusted() {
        platform::request_trust();
        return Err(UNTRUSTED.into());
    }
    if let Some(background) = target.background {
        // A failed write may already have reached the provider. Never replay it
        // through keyboard input, which could duplicate text or write into B.
        log::info!("[VoiceInput] input route=captured_control");
        return background.insert(text, cancel);
    }
    // No editable control was exposed (macOS WeChat, for one): type into the
    // original app only while it is still in front. On macOS that is the app itself.
    if target
        .foreground
        .is_some_and(|original| original.matches(native::input_target()))
    {
        // Never type a transcript into a password field.
        #[cfg(target_os = "macos")]
        if target.foreground.is_some_and(platform::secure_input) {
            log::info!("[VoiceInput] input skipped secure_input=true");
            return Err(UNAVAILABLE.into());
        }
        log::info!("[VoiceInput] input route=foreground");
        return native::insert(text, target.foreground, cancel);
    }
    Err(UNAVAILABLE.into())
}

struct Edit {
    original: String,
    range: Range<usize>,
}

#[cfg(any(windows, test))]
fn confirm_write(
    expected: &str,
    acknowledged: bool,
    mut read: impl FnMut() -> Option<String>,
) -> Result<(), String> {
    // An accessibility setter acknowledges the edit itself; its cached value
    // may lag behind. Native message delivery alone is not this confirmation.
    if acknowledged {
        return Ok(());
    }
    use std::time::{Duration, Instant};
    let started = Instant::now();
    let normalize = |text: &str| text.replace("\r\n", "\n").replace('\r', "\n");
    let expected = normalize(expected);
    loop {
        if let Some(actual) = read() {
            let actual = normalize(&actual);
            // Accessibility providers may expose their document terminator.
            if actual == expected || actual.strip_suffix('\n') == Some(expected.as_str()) {
                return Ok(());
            }
        }
        // Only repeat reads: a provider can apply a write before its cached
        // value catches up, or even after its acknowledgement times out.
        if started.elapsed() >= Duration::from_millis(400) {
            return Err(UNAVAILABLE.into());
        }
        std::thread::sleep(Duration::from_millis(40));
    }
}

impl Edit {
    fn new(original: String, range: Range<usize>) -> Option<Self> {
        (original.len() <= MAX_FIELD_BYTES
            && range.start <= range.end
            && original.is_char_boundary(range.start)
            && original.is_char_boundary(range.end))
        .then_some(Self { original, range })
    }

    #[cfg(any(windows, target_os = "macos", test))]
    fn utf16(original: String, start: usize, end: usize) -> Option<Self> {
        let boundary = |offset| {
            let mut units = 0;
            for (byte, character) in original.char_indices() {
                if units == offset {
                    return Some(byte);
                }
                units += character.len_utf16();
            }
            (units == offset).then_some(original.len())
        };
        let range = boundary(start)?..boundary(end)?;
        Self::new(original, range)
    }

    #[cfg(any(target_os = "linux", test))]
    fn characters(original: String, start: usize, end: usize) -> Option<Self> {
        let boundary = |offset| {
            original
                .char_indices()
                .map(|(byte, _)| byte)
                .chain(Some(original.len()))
                .nth(offset)
        };
        let range = boundary(start)?..boundary(end)?;
        Self::new(original, range)
    }

    #[cfg(any(windows, test))]
    fn surrounding(original: String, before: &str, selected: &str, after: &str) -> Option<Self> {
        let normalize = |text: &str| text.replace("\r\n", "\n").replace('\r', "\n");
        let before = normalize(before);
        let selected = normalize(selected);
        let combined = format!("{before}{selected}{}", normalize(after));
        let normalized = normalize(&original);
        // Some text providers append a document terminator outside the value.
        if combined != normalized && combined.strip_suffix('\n') != Some(normalized.as_str()) {
            return None;
        }
        let boundary = |offset: usize| {
            let mut remaining = offset;
            let mut characters = original.char_indices().peekable();
            while let Some((byte, character)) = characters.next() {
                if remaining == 0 {
                    return Some(byte);
                }
                let length = if character == '\r' {
                    if characters.peek().is_some_and(|(_, next)| *next == '\n') {
                        characters.next();
                    }
                    1
                } else {
                    character.len_utf8()
                };
                remaining = remaining.checked_sub(length)?;
            }
            (remaining == 0).then_some(original.len())
        };
        let range = boundary(before.len())?..boundary(before.len() + selected.len())?;
        Self::new(original, range)
    }

    fn replace(&self, current: &str, text: &str) -> Result<String, String> {
        if current != self.original {
            return Err(UNAVAILABLE.into());
        }
        let mut output = self.original.clone();
        output.replace_range(self.range.clone(), text);
        Ok(output)
    }
}

#[cfg(test)]
#[path = "dictation_target/tests.rs"]
mod tests;
