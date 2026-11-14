use super::*;

#[test]
fn confirmation_allows_delayed_provider_state_and_document_line_endings() {
    let mut values = [Some("old".into()), None, Some("new\r\nsecond\r".into())].into_iter();
    assert_eq!(
        confirm_write("new\nsecond", false, || values.next().flatten()),
        Ok(())
    );
    assert!(values.next().is_none());
}

#[test]
fn confirmation_does_not_accept_partial_or_missing_text() {
    assert_eq!(
        confirm_write("complete result", false, || Some("complete".into())),
        Err(UNAVAILABLE.into())
    );
    assert_eq!(
        confirm_write("result", false, || None),
        Err(UNAVAILABLE.into())
    );
}

#[test]
fn acknowledged_automation_write_does_not_depend_on_stale_or_missing_readback() {
    for cached in [Some("old cached value"), None] {
        let mut reads = 0;
        assert_eq!(
            confirm_write("inserted text", true, || {
                reads += 1;
                cached.map(str::to_string)
            }),
            Ok(())
        );
        assert_eq!(reads, 0);
    }
}

#[test]
fn remembered_utf16_caret_inserts_before_the_original_suffix() {
    let original = "开始🙂，最后一句";
    let edit = Edit::utf16(original.into(), 4, 4).unwrap();
    assert_eq!(
        edit.replace(original, "新增\n第二行").unwrap(),
        "开始🙂新增\n第二行，最后一句"
    );
    assert!(Edit::utf16(original.into(), 3, 3).is_none());
    assert!(Edit::utf16(original.into(), 100, 100).is_none());
}

#[test]
fn selection_replaces_only_the_selected_characters_and_rejects_later_edits() {
    let original = "before🙂old café\nafter";
    let edit = Edit::characters(original.into(), 7, 10).unwrap();
    assert_eq!(
        edit.replace(original, "新内容").unwrap(),
        "before🙂新内容 café\nafter"
    );
    assert_eq!(
        edit.replace("user edited this while waiting", "result"),
        Err(UNAVAILABLE.into())
    );
    assert_eq!(edit.replace("", "result"), Err(UNAVAILABLE.into()));
}

#[test]
fn provider_line_endings_map_back_without_rewriting_the_original_document() {
    let original = "first\r\n🙂second\r\nlast";
    let edit = Edit::surrounding(original.into(), "first\n🙂", "second", "\nlast\n").unwrap();
    assert_eq!(
        edit.replace(original, "new").unwrap(),
        "first\r\n🙂new\r\nlast"
    );
    assert!(Edit::surrounding(original.into(), "different", "second", "\nlast").is_none());
}

#[test]
fn cancellation_and_missing_targets_never_fall_through_to_current_focus() {
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        insert("result", None, &cancel),
        Err("voice.error.cancelled".into())
    );
    assert_eq!(
        insert("result", None, &CancellationToken::new()),
        Err(UNAVAILABLE.into())
    );
    assert_eq!(
        insert("before\0after", None, &CancellationToken::new()),
        Err("voice.error.insert".into())
    );
}

// macOS answers an untrusted process with the Accessibility error first.
#[cfg(not(target_os = "macos"))]
#[test]
fn an_unavailable_background_target_never_types_into_another_window() {
    let target = Target {
        foreground: Some(InputTarget {
            window: u64::MAX,
            focus: Some(u64::MAX),
        }),
        background: None,
    };
    assert_eq!(
        insert(
            "do not put this in B",
            Some(target),
            &CancellationToken::new()
        ),
        Err(UNAVAILABLE.into())
    );
}

#[test]
fn an_app_only_target_types_only_while_that_app_is_still_in_front() {
    // macOS knows the original app but not its field.
    let original = InputTarget {
        window: 42,
        focus: None,
    };
    assert!(original.matches(Some(InputTarget {
        window: 42,
        focus: None,
    })));
    assert!(!original.matches(Some(InputTarget {
        window: 7,
        focus: None,
    })));
    assert!(!original.matches(None));
}

#[test]
fn paste_confirmation_requires_new_text_and_normalizes_line_endings() {
    assert!(insertion_observed("draft", "draft语音🙂", "语音🙂"));
    assert!(insertion_observed(
        "one\r\n",
        "one\r\ntwo\r\nthree",
        "two\nthree"
    ));
    assert!(insertion_observed("repeat", "repeat repeat", "repeat"));
    assert!(!insertion_observed("already here", "already here", "here"));
    assert!(!insertion_observed("before", "before par", "partial"));
    assert!(!insertion_observed("old", "different", "result"));
    assert!(!insertion_observed("", "", ""));
    assert!(!insertion_observed("", "  ", "  "));
}

#[test]
fn paste_confirmation_polls_reads_without_repeating_the_write() {
    let mut reads = 0;
    assert_eq!(
        confirm_paste(&CancellationToken::new(), &mut || {
            reads += 1;
            reads == 3
        }),
        Ok(())
    );
    assert_eq!(reads, 3);
}

#[test]
fn paste_confirmation_cancellation_prevents_further_reads() {
    let cancel = CancellationToken::new();
    let mut reads = 0;
    assert_eq!(
        confirm_paste(&cancel, &mut || {
            reads += 1;
            cancel.cancel();
            false
        }),
        Err("voice.error.cancelled".into())
    );
    assert_eq!(reads, 1);
}

#[test]
fn missing_paste_confirmation_is_reported_without_retrying_input() {
    assert_eq!(
        confirm_paste(&CancellationToken::new(), &mut || false),
        Err("voice.error.unconfirmed".into())
    );
}
