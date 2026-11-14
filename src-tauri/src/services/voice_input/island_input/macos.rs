use objc2::{
    ffi, msg_send,
    runtime::{AnyClass, AnyObject, Bool, Imp, Sel},
    sel,
};
use std::sync::OnceLock;

type PanelQuery = unsafe extern "C-unwind" fn(&AnyObject, Sel) -> Bool;

static PANEL_QUERY: OnceLock<PanelQuery> = OnceLock::new();
static PANEL_QUERY_INSTALLED: OnceLock<bool> = OnceLock::new();
static ISLAND_MARKER: u8 = 0;

extern "C-unwind" fn is_nonactivating_panel(window: &AnyObject, selector: Sel) -> Bool {
    // An association follows this instance through KVO's own subclass changes
    // and is removed with the window, without retaining the window itself.
    let marker =
        unsafe { ffi::objc_getAssociatedObject(window, (&raw const ISLAND_MARKER).cast()) };
    if !marker.is_null() {
        return Bool::YES;
    }
    let original = PANEL_QUERY.get().expect("installed panel query");
    unsafe { original(window, selector) }
}

fn install_panel_query(class: &'static AnyClass) -> bool {
    *PANEL_QUERY_INSTALLED.get_or_init(|| {
        let selector = sel!(_isNonactivatingPanel);
        let Some(method) = class.instance_method(selector) else {
            return false;
        };
        // SAFETY: the inherited AppKit method and our implementation both
        // take (id, SEL) and return Objective-C BOOL. Copy its type encoding.
        let original: PanelQuery = unsafe { std::mem::transmute(method.implementation()) };
        // Publish the fallback before making the override visible to any
        // other thread querying an ordinary window during installation.
        PANEL_QUERY
            .set(original)
            .expect("panel query initialized once");
        let replacement: Imp = unsafe {
            std::mem::transmute(is_nonactivating_panel as extern "C-unwind" fn(_, _) -> Bool)
        };
        let installed = unsafe {
            ffi::class_addMethod(
                (class as *const AnyClass).cast_mut(),
                selector,
                replacement,
                ffi::method_getTypeEncoding(method),
            )
        };
        installed.as_bool()
    })
}

fn mark_island(window: &AnyObject) {
    let pointer = (window as *const AnyObject).cast_mut();
    // SAFETY: this non-owning self marker is only read while the receiver is
    // alive. The runtime owns the association and removes it on deallocation.
    unsafe {
        ffi::objc_setAssociatedObject(
            pointer,
            (&raw const ISLAND_MARKER).cast(),
            pointer,
            ffi::OBJC_ASSOCIATION_ASSIGN,
        );
    }
}

pub(super) fn configure(window: &tauri::WebviewWindow) -> tauri::Result<()> {
    let handle = window.clone();
    window.run_on_main_thread(move || {
        let Ok(island) = handle.ns_window() else {
            return;
        };
        // SAFETY: the live window is accessed only on AppKit's main thread.
        let island = unsafe { &*(island as *const AnyObject) };
        // Install on the original window class, leaving KVO's runtime subclass
        // and observation metadata intact. Other windows retain AppKit's query.
        // The builder's focusable(false) already handles key/main-window refusal.
        let class: &'static AnyClass = unsafe { msg_send![island, class] };
        if install_panel_query(class) {
            mark_island(island);
        } else {
            log::warn!("[VoiceInput] the island cannot configure nonactivation");
        }
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
    use objc2::{rc::Retained, runtime::ClassBuilder};
    use std::sync::atomic::{AtomicUsize, Ordering};

    static EVENTS: AtomicUsize = AtomicUsize::new(0);
    static OBSERVATIONS: AtomicUsize = AtomicUsize::new(0);

    extern "C-unwind" fn native_event(_: &AnyObject, _: Sel, _: &AnyObject) {
        EVENTS.fetch_add(1, Ordering::SeqCst);
    }

    extern "C-unwind" fn no(_: &AnyObject, _: Sel) -> Bool {
        Bool::NO
    }

    extern "C-unwind" fn saved_state(this: &AnyObject, _: Sel) -> u64 {
        let state = this.class().instance_variable(c"savedState").unwrap();
        unsafe { *state.load::<u64>(this) }
    }

    extern "C-unwind" fn set_saved_state(this: &AnyObject, _: Sel, value: u64) {
        let state = this.class().instance_variable(c"savedState").unwrap();
        unsafe { state.load_ptr::<u64>(this).write(value) };
    }

    extern "C-unwind" fn observe(
        _: &AnyObject,
        _: Sel,
        _: &AnyObject,
        _: &AnyObject,
        _: &AnyObject,
        _: *mut std::ffi::c_void,
    ) {
        OBSERVATIONS.fetch_add(1, Ordering::SeqCst);
    }

    #[test]
    fn nonactivating_window_preserves_observer_class_and_other_windows() {
        let result = objc2::exception::catch(|| {
            let mut native = ClassBuilder::new(
                c"EchoBirdVoiceIslandTestNativeWindow",
                AnyClass::get(c"NSObject").unwrap(),
            )
            .unwrap();
            unsafe {
                native.add_method(
                    sel!(_isNonactivatingPanel),
                    no as extern "C-unwind" fn(_, _) -> _,
                );
                native.add_method(
                    sel!(canBecomeKeyWindow),
                    no as extern "C-unwind" fn(_, _) -> _,
                );
                native.add_method(
                    sel!(canBecomeMainWindow),
                    no as extern "C-unwind" fn(_, _) -> _,
                );
                native.add_method(
                    sel!(sendEvent:),
                    native_event as extern "C-unwind" fn(_, _, _),
                );
            }
            let native = native.register();
            let mut base = ClassBuilder::new(c"EchoBirdVoiceIslandTestWindow", native).unwrap();
            base.add_ivar::<u64>(c"savedState");
            unsafe {
                base.add_method(
                    sel!(savedState),
                    saved_state as extern "C-unwind" fn(_, _) -> _,
                );
                base.add_method(
                    sel!(setSavedState:),
                    set_saved_state as extern "C-unwind" fn(_, _, _),
                );
            }
            let base = base.register();
            let mut observer_class = ClassBuilder::new(
                c"EchoBirdVoiceIslandTestObserver",
                AnyClass::get(c"NSObject").unwrap(),
            )
            .unwrap();
            unsafe {
                observer_class.add_method(
                    sel!(observeValueForKeyPath:ofObject:change:context:),
                    observe as extern "C-unwind" fn(_, _, _, _, _, _),
                );
            }
            let observer_class = observer_class.register();
            let observer: Retained<AnyObject> = unsafe { msg_send![observer_class, new] };
            let additional_observer: Retained<AnyObject> =
                unsafe { msg_send![observer_class, new] };
            let island: Retained<AnyObject> = unsafe { msg_send![base, new] };
            let other: Retained<AnyObject> = unsafe { msg_send![base, new] };
            let key_path = objc2_foundation::ns_string!("savedState");
            let _: () = unsafe {
                msg_send![&*island, addObserver: &*observer, forKeyPath: key_path, options: 1usize, context: std::ptr::null_mut::<std::ffi::c_void>()]
            };
            let runtime_class = island.class();
            assert_ne!(runtime_class, base);
            let state = base.instance_variable(c"savedState").unwrap();
            unsafe { state.load_ptr::<u64>(&island).write(42) };
            let before: &'static AnyClass = unsafe { msg_send![&*island, class] };
            assert_eq!(before, base);
            let panel: Bool = unsafe { msg_send![&*island, _isNonactivatingPanel] };
            assert!(!panel.as_bool());
            assert!(install_panel_query(base));
            mark_island(&island);
            assert!(install_panel_query(base));
            mark_island(&island);
            assert_eq!(island.class(), runtime_class);
            let after: &'static AnyClass = unsafe { msg_send![&*island, class] };
            assert_eq!(after, before);
            assert_eq!(unsafe { *state.load::<u64>(&island) }, 42);
            let _: () = unsafe { msg_send![&*island, setSavedState: 43u64] };
            assert_eq!(OBSERVATIONS.load(Ordering::SeqCst), 1);
            // Adding another real Foundation observer exercises the system sharing
            // path that crashed when an extra class was installed above KVO.
            let _: () = unsafe {
                msg_send![&*island, addObserver: &*additional_observer, forKeyPath: key_path, options: 1usize, context: std::ptr::null_mut::<std::ffi::c_void>()]
            };
            let _: () = unsafe { msg_send![&*island, setSavedState: 44u64] };
            assert_eq!(OBSERVATIONS.load(Ordering::SeqCst), 3);
            assert_eq!(unsafe { *state.load::<u64>(&island) }, 44);
            let panel: Bool = unsafe { msg_send![&*island, _isNonactivatingPanel] };
            assert!(panel.as_bool());
            let other_panel: Bool = unsafe { msg_send![&*other, _isNonactivatingPanel] };
            assert!(!other_panel.as_bool());
            let key: Bool = unsafe { msg_send![&*island, canBecomeKeyWindow] };
            let main: Bool = unsafe { msg_send![&*island, canBecomeMainWindow] };
            assert!(!key.as_bool());
            assert!(!main.as_bool());
            let _: () = unsafe { msg_send![&*island, sendEvent: &*other] };
            assert_eq!(EVENTS.load(Ordering::SeqCst), 1);
            let _: () =
                unsafe { msg_send![&*island, removeObserver: &*observer, forKeyPath: key_path] };
            let _: () = unsafe {
                msg_send![&*island, removeObserver: &*additional_observer, forKeyPath: key_path]
            };
        });
        assert!(result.is_ok(), "Foundation KVO fixture failed: {result:?}");
    }
}
