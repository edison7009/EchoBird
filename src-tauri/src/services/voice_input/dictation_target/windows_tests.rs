use super::*;
use windows::core::{w, PCWSTR};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DestroyWindow, DispatchMessageW, GetForegroundWindow, PeekMessageW,
    TranslateMessage, ES_AUTOVSCROLL, ES_MULTILINE, MSG, PM_REMOVE, WINDOW_EX_STYLE, WINDOW_STYLE,
    WM_SETTEXT, WS_CHILD,
};

struct Field {
    window: HWND,
    input: HWND,
}

impl Field {
    fn new(text: &str) -> Self {
        // Hidden, test-owned controls only. No activation, user clipboard or
        // keyboard events are needed to exercise the real Win32 edit path.
        let window = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!(""),
                WINDOW_STYLE(0),
                0,
                0,
                200,
                100,
                None,
                None,
                None,
                None,
            )
            .unwrap()
        };
        let text: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
        let input = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("EDIT"),
                PCWSTR(text.as_ptr()),
                WS_CHILD | WINDOW_STYLE((ES_MULTILINE | ES_AUTOVSCROLL) as u32),
                0,
                0,
                200,
                100,
                Some(window),
                None,
                None,
                None,
            )
            .unwrap()
        };
        Self { window, input }
    }

    fn target(&self, start: usize, end: usize) -> Target {
        send(
            self.input.0 as u64,
            0x00B1,
            WPARAM(start),
            LPARAM(end as isize),
        )
        .unwrap();
        capture(Some(InputTarget {
            window: self.window.0 as u64,
            focus: Some(self.input.0 as u64),
        }))
        .unwrap()
    }

    fn text(&self) -> String {
        value(self.input.0 as u64).unwrap()
    }
}

impl Drop for Field {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.window) };
    }
}

#[test]
fn displayed_hint_values_are_not_safe_background_drafts() {
    for hint in ["Type here", "在此输入", "在此輸入", "ここに入力"] {
        assert!(ambiguous_value(hint, hint, ""));
        assert!(ambiguous_value(hint, "Input", hint));
        assert!(!ambiguous_value("", hint, hint));
        let draft = format!("{hint}\nactual draft🙂");
        assert!(!ambiguous_value(&draft, hint, hint));
        assert!(!ambiguous_value(&draft, "", ""));
    }
}

#[test]
fn web_values_never_require_a_matching_placeholder_label_to_use_paste() {
    assert!(!ambiguous_value("visible hint\n", "", ""));
    assert!(web_editor("Chrome", "", ""));
    assert!(web_editor("", "textbox", ""));
    assert!(web_editor("", "", "Chrome_WidgetWin_1"));
    assert!(web_editor("Chrome", "textbox", "Chrome_WidgetWin_1"));
    assert!(!web_editor("Win32", "", "Edit"));
    assert!(!web_editor("WPF", "", "HwndWrapper"));
}

#[test]
fn real_text_matching_a_label_is_not_stripped_from_native_edit_controls() {
    let field = Field::new("Type here");
    field
        .target(9, 9)
        .insert(" — dictated text", &CancellationToken::new())
        .unwrap();
    assert_eq!(field.text(), "Type here — dictated text");
}

#[test]
fn automation_capture_accepts_only_the_original_window_even_with_the_same_process() {
    let a = Field::new("original");
    let b = Field::new("other window");
    let owner = a.window.0 as u64;
    let original = a.input.0 as usize;
    let other = b.input.0 as usize;
    let worker = std::thread::spawn(move || unsafe {
        let _apartment = Apartment::new().unwrap();
        let automation: IUIAutomation2 =
            CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER).unwrap();
        automation.SetAutoSetFocus(false).unwrap();
        // Allow a cold UIA provider to start on a loaded runner. Production
        // capture keeps its short deadlines; this fixture verifies ownership.
        automation.SetConnectionTimeout(2_000).unwrap();
        automation.SetTransactionTimeout(2_500).unwrap();
        let original = automation
            .ElementFromHandle(HWND(original as *mut _))
            .unwrap();
        let other = automation.ElementFromHandle(HWND(other as *mut _)).unwrap();
        let ownership = (
            automation_in_window(&automation, &original, owner),
            automation_in_window(&automation, &other, owner),
        );
        (ownership, AgileReference::new(&original).unwrap())
    });
    // Service only this test thread's hidden controls while UIA queries them.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    while !worker.is_finished() && std::time::Instant::now() < deadline {
        let mut message = MSG::default();
        unsafe {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(worker.is_finished(), "UIA fixture timed out");
    let (ownership, reference) = worker.join().unwrap();
    assert_eq!(ownership, (Some(()), None));
    let target = Target {
        window: owner,
        pid: std::process::id(),
        control: Control::ForegroundOnly {
            reference,
            input: InputTarget {
                window: owner,
                focus: Some(original as u64),
            },
            original: "original".into(),
        },
    };
    assert_eq!(
        target.insert("must not reach another field", &CancellationToken::new()),
        Err(UNAVAILABLE.into())
    );
    assert_eq!(a.text(), "original");
    assert_eq!(b.text(), "other window");
}

#[test]
fn background_write_does_not_activate_a_test_window_and_uses_the_saved_unicode_caret() {
    let a = Field::new("前🙂后");
    let b = Field::new("B must remain unchanged");
    let target = a.target(3, 3);
    // Even if A's current selection later moves, use the captured range.
    send(a.input.0 as u64, 0x00B1, WPARAM(0), LPARAM(0)).unwrap();
    target
        .insert("第一行\n第二行", &CancellationToken::new())
        .unwrap();
    assert_eq!(a.text(), "前🙂第一行\r\n第二行后");
    assert_eq!(b.text(), "B must remain unchanged");
    let foreground = unsafe { GetForegroundWindow() };
    assert_ne!(foreground, a.window);
    assert_ne!(foreground, b.window);
}

#[test]
fn background_selection_replacement_preserves_the_surrounding_long_document() {
    let original = format!(
        "{}old{}",
        "before🙂\r\n".repeat(400),
        "\r\nafter".repeat(400)
    );
    let a = Field::new(&original);
    let start = "before🙂\r\n".repeat(400).encode_utf16().count();
    a.target(start, start + 3)
        .insert("new", &CancellationToken::new())
        .unwrap();
    assert_eq!(a.text(), original.replacen("old", "new", 1));
}

#[test]
fn changed_closed_and_cancelled_targets_do_not_write_to_another_field() {
    let a = Field::new("original");
    let b = Field::new("untouched");
    let target = a.target(0, 0);
    let changed: Vec<u16> = "changed".encode_utf16().chain(Some(0)).collect();
    send(
        a.input.0 as u64,
        WM_SETTEXT,
        WPARAM(0),
        LPARAM(changed.as_ptr() as isize),
    )
    .unwrap();
    assert_eq!(
        target.insert("result", &CancellationToken::new()),
        Err(UNAVAILABLE.into())
    );
    assert_eq!(a.text(), "changed");
    let target = a.target(0, 0);
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        target.insert("result", &cancel),
        Err("voice.error.cancelled".into())
    );
    assert_eq!(a.text(), "changed");
    let target = a.target(0, 0);
    drop(a);
    assert_eq!(
        target.insert("result", &CancellationToken::new()),
        Err(UNAVAILABLE.into())
    );
    assert_eq!(b.text(), "untouched");
}

#[test]
fn foreground_paste_accepts_edits_around_the_captured_text_but_not_replaced_content() {
    assert!(kept_original("", "anything"));
    assert!(kept_original("draft", "draft"));
    assert!(kept_original("draft", "draft typed while speaking"));
    assert!(kept_original(
        "line one\r\nline two",
        "line one\nline two\n"
    ));
    assert!(kept_original("  draft\n", "prefix draft"));
    assert!(!kept_original("conversation A draft", ""));
    assert!(!kept_original(
        "conversation A draft",
        "conversation B draft"
    ));
}
