use objc2::{
    msg_send,
    runtime::{AnyClass, AnyObject, Bool, ClassBuilder, Sel},
    sel,
};
use std::sync::OnceLock;

extern "C-unwind" fn no(_: &AnyObject, _: Sel) -> Bool {
    Bool::NO
}

extern "C-unwind" fn yes(_: &AnyObject, _: Sel) -> Bool {
    Bool::YES
}

struct IslandClass {
    class: &'static AnyClass,
    event_superclass: &'static AnyClass,
}

static CLASS: OnceLock<IslandClass> = OnceLock::new();

extern "C-unwind" fn send_event(this: &AnyObject, _: Sel, event: &AnyObject) {
    // Tao's inherited sendEvent: dynamically asks this window for its superclass.
    // After subclassing, that resolves back to Tao and re-enters the same method.
    // Preserve Tao's background dragging, then dispatch to its original parent.
    let event_type: usize = unsafe { msg_send![event, type] };
    if event_type == 1 {
        let movable: Bool = unsafe { msg_send![this, isMovableByWindowBackground] };
        if movable.as_bool() {
            let _: () = unsafe { msg_send![this, performWindowDragWithEvent: event] };
        }
    }
    let parent = CLASS
        .get()
        .expect("registered island class")
        .event_superclass;
    let _: () = unsafe { msg_send![super(this, parent), sendEvent: event] };
}

fn nonactivating_class(superclass: &'static AnyClass) -> &'static AnyClass {
    CLASS
        .get_or_init(|| {
            let mut class = ClassBuilder::new(c"EchoBirdVoiceIslandWindow", superclass).unwrap();
            // SAFETY: these overrides have the same Objective-C signatures as the
            // inherited window methods. No ivars are added; dragging, delegates and
            // the window library's focusable state remain on the original class.
            unsafe {
                class.add_method(
                    sel!(sendEvent:),
                    send_event as extern "C-unwind" fn(_, _, _),
                );
                class.add_method(
                    sel!(canBecomeKeyWindow),
                    no as extern "C-unwind" fn(_, _) -> _,
                );
                class.add_method(
                    sel!(canBecomeMainWindow),
                    no as extern "C-unwind" fn(_, _) -> _,
                );
                // AppKit checks this independently of whether a window can become key.
                class.add_method(
                    sel!(_isNonactivatingPanel),
                    yes as extern "C-unwind" fn(_, _) -> _,
                );
            }
            IslandClass {
                class: class.register(),
                event_superclass: superclass.superclass().expect("window parent class"),
            }
        })
        .class
}

pub(super) fn configure(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let handle = window.clone();
    window.run_on_main_thread(move || {
        let Ok(island) = handle.ns_window() else {
            return;
        };
        // SAFETY: the live window is accessed only on AppKit's main thread.
        // Its replacement class is an ivar-free subclass
        // of its original class, preserving the allocation and inherited state.
        let island = unsafe { &*(island as *const AnyObject) };
        let class = nonactivating_class(island.class());
        if island.class() != class {
            unsafe { AnyObject::set_class(island, class) };
        }
        // The window was initialized before the override was installed, so also
        // update the window server's activation tag before its first display.
        let supported: Bool =
            unsafe { msg_send![island, respondsToSelector: sel!(_setPreventsActivation:)] };
        if supported.as_bool() {
            let _: () = unsafe { msg_send![island, _setPreventsActivation: Bool::YES] };
        } else {
            log::warn!("[VoiceInput] the island cannot set its activation tag");
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::rc::Retained;
    use std::sync::atomic::{AtomicUsize, Ordering};

    static BASE_EVENTS: AtomicUsize = AtomicUsize::new(0);
    static NATIVE_EVENTS: AtomicUsize = AtomicUsize::new(0);
    static DRAGS: AtomicUsize = AtomicUsize::new(0);
    static EVENT_TYPE: AtomicUsize = AtomicUsize::new(1);

    extern "C-unwind" fn native_event(_: &AnyObject, _: Sel, _: &AnyObject) {
        NATIVE_EVENTS.fetch_add(1, Ordering::SeqCst);
    }

    extern "C-unwind" fn window_event(this: &AnyObject, _: Sel, event: &AnyObject) {
        BASE_EVENTS.fetch_add(1, Ordering::SeqCst);
        let parent: &AnyClass = unsafe { msg_send![this, superclass] };
        let _: () = unsafe { msg_send![super(this, parent), sendEvent: event] };
    }

    extern "C-unwind" fn drag(_: &AnyObject, _: Sel, _: &AnyObject) {
        DRAGS.fetch_add(1, Ordering::SeqCst);
    }

    extern "C-unwind" fn event_type(_: &AnyObject, _: Sel) -> usize {
        EVENT_TYPE.load(Ordering::SeqCst)
    }

    #[test]
    fn nonactivating_window_preserves_native_state_and_rejects_keyboard_focus() {
        let mut native = ClassBuilder::new(
            c"EchoBirdVoiceIslandTestNativeWindow",
            AnyClass::get(c"NSObject").unwrap(),
        )
        .unwrap();
        unsafe {
            native.add_method(
                sel!(sendEvent:),
                native_event as extern "C-unwind" fn(_, _, _),
            );
        }
        let mut base =
            ClassBuilder::new(c"EchoBirdVoiceIslandTestWindow", native.register()).unwrap();
        base.add_ivar::<u64>(c"savedState");
        unsafe {
            base.add_method(
                sel!(sendEvent:),
                window_event as extern "C-unwind" fn(_, _, _),
            );
            base.add_method(
                sel!(isMovableByWindowBackground),
                yes as extern "C-unwind" fn(_, _) -> _,
            );
            base.add_method(
                sel!(performWindowDragWithEvent:),
                drag as extern "C-unwind" fn(_, _, _),
            );
            base.add_method(
                sel!(canBecomeKeyWindow),
                yes as extern "C-unwind" fn(_, _) -> _,
            );
            base.add_method(
                sel!(canBecomeMainWindow),
                yes as extern "C-unwind" fn(_, _) -> _,
            );
            base.add_method(
                sel!(_isNonactivatingPanel),
                no as extern "C-unwind" fn(_, _) -> _,
            );
        }
        let base = base.register();
        let object: Retained<AnyObject> = unsafe { msg_send![base, new] };
        let mut event_class = ClassBuilder::new(
            c"EchoBirdVoiceIslandTestEvent",
            AnyClass::get(c"NSObject").unwrap(),
        )
        .unwrap();
        unsafe {
            event_class.add_method(sel!(type), event_type as extern "C-unwind" fn(_, _) -> _);
        }
        let event: Retained<AnyObject> = unsafe { msg_send![event_class.register(), new] };
        let _: () = unsafe { msg_send![&*object, sendEvent: &*event] };
        assert_eq!(BASE_EVENTS.load(Ordering::SeqCst), 1);
        assert_eq!(NATIVE_EVENTS.load(Ordering::SeqCst), 1);
        let state = base.instance_variable(c"savedState").unwrap();
        // SAFETY: this fixture has sole ownership and no outstanding borrows.
        unsafe { state.load_ptr::<u64>(&object).write(42) };
        // Positive control: the unmodified window accepts activation and focus.
        let key: Bool = unsafe { msg_send![&*object, canBecomeKeyWindow] };
        let panel: Bool = unsafe { msg_send![&*object, _isNonactivatingPanel] };
        assert!(key.as_bool());
        assert!(!panel.as_bool());
        let class = nonactivating_class(base);
        assert_eq!(class.superclass(), Some(base));
        assert_eq!(class.instance_size(), base.instance_size());
        unsafe { AnyObject::set_class(&object, class) };
        let key: Bool = unsafe { msg_send![&*object, canBecomeKeyWindow] };
        let main: Bool = unsafe { msg_send![&*object, canBecomeMainWindow] };
        let panel: Bool = unsafe { msg_send![&*object, _isNonactivatingPanel] };
        assert!(!key.as_bool());
        assert!(!main.as_bool());
        assert!(panel.as_bool());
        assert_eq!(unsafe { *state.load::<u64>(&object) }, 42);
        assert_eq!(nonactivating_class(base), class);
        // Mouse-down preserves native dragging and reaches AppKit once, without
        // re-entering the inherited dynamic-superclass handler. Other events do not drag.
        let _: () = unsafe { msg_send![&*object, sendEvent: &*event] };
        EVENT_TYPE.store(2, Ordering::SeqCst);
        let _: () = unsafe { msg_send![&*object, sendEvent: &*event] };
        assert_eq!(BASE_EVENTS.load(Ordering::SeqCst), 1);
        assert_eq!(NATIVE_EVENTS.load(Ordering::SeqCst), 3);
        assert_eq!(DRAGS.load(Ordering::SeqCst), 1);
    }
}
