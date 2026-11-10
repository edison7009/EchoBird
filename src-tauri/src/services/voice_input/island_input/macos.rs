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

fn nonactivating_class(superclass: &'static AnyClass) -> &'static AnyClass {
    static CLASS: OnceLock<&AnyClass> = OnceLock::new();
    CLASS.get_or_init(|| {
        let mut class = ClassBuilder::new(c"EchoBirdVoiceIslandWindow", superclass).unwrap();
        // SAFETY: these overrides have the same Objective-C signatures as the
        // inherited window methods. No ivars are added; dragging, delegates and
        // the window library's focusable state remain on the original class.
        unsafe {
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
        class.register()
    })
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

    #[test]
    fn nonactivating_window_preserves_native_state_and_rejects_keyboard_focus() {
        let mut base = ClassBuilder::new(
            c"EchoBirdVoiceIslandTestWindow",
            AnyClass::get(c"NSObject").unwrap(),
        )
        .unwrap();
        base.add_ivar::<u64>(c"savedState");
        unsafe {
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
    }
}
