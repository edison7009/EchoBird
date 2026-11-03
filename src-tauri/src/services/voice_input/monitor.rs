use super::{app_icon, app_icon_cache, native, publish, shared};
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::time::{Duration, Instant};
use tauri::AppHandle;
#[cfg(any(windows, target_os = "macos"))]
use tauri::Manager;

pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let voice = shared(&app);
        let (requests, receiver) = mpsc::channel::<(u64, app_icon::Target)>();
        let icon_app = app.clone();
        let icon_voice = voice.clone();
        std::thread::spawn(move || {
            let mut icons = app_icon_cache::Cache::default();
            while let Ok(mut request) = receiver.recv() {
                // Skip intermediate windows while a desktop icon lookup was busy.
                while let Ok(latest) = receiver.try_recv() {
                    request = latest;
                }
                let (generation, window) = request;
                if icon_voice.stopped.load(Ordering::Acquire) {
                    break;
                }
                if icon_voice.runtime.lock().unwrap().app_icon_generation != generation {
                    continue;
                }
                let key = app_icon::cache_key(&window);
                let icon = icons.read(key.clone(), Instant::now(), || {
                    let icon = app_icon::read_target(&window)?;
                    (app_icon::cache_key(&window) == key).then_some(icon)
                });
                if icon_voice.stopped.load(Ordering::Acquire) {
                    break;
                }
                let accepted = icon_voice
                    .runtime
                    .lock()
                    .unwrap()
                    .finish_app_icon(generation, icon);
                if accepted {
                    publish(&icon_app);
                }
            }
        });
        let mut enabled_before = false;
        let mut escape = None;
        let mut pressed_before = false;
        let mut previous_window = None;
        let mut checked_icon = Instant::now();
        let mut foreground = None;
        #[cfg(any(windows, target_os = "macos"))]
        let mut cursor = super::island_input::CursorTracker::default();
        while !voice.stopped.load(Ordering::Acquire) {
            let (enabled, zoom) = {
                let saved = voice.saved.lock().unwrap();
                (
                    super::island_visible(&saved.config),
                    saved.config.island_zoom(),
                )
            };
            if enabled && !enabled_before {
                foreground = Some(app_icon::Monitor::new());
                escape = native::ModifierMonitor::escape();
                previous_window = None;
                checked_icon = Instant::now() - Duration::from_secs(1);
            }
            if enabled {
                #[cfg(any(windows, target_os = "macos"))]
                if let Some(window) = app.get_webview_window("voice-island") {
                    cursor.update(&window, zoom);
                }
                let pressed = escape.as_ref().is_some_and(native::ModifierMonitor::down);
                if pressed && !pressed_before {
                    super::escape(&app);
                }
                pressed_before = pressed;
                if checked_icon.elapsed() >= Duration::from_millis(250) {
                    checked_icon = Instant::now();
                    let window = foreground.as_ref().and_then(app_icon::Monitor::current);
                    if previous_window != Some(window.clone()) {
                        previous_window = Some(window.clone());
                        let generation = voice
                            .runtime
                            .lock()
                            .unwrap()
                            .begin_app_icon(window.is_some());
                        publish(&app);
                        if let Some(window) = window {
                            let _ = requests.send((generation, window));
                        }
                    }
                }
            } else {
                foreground = None;
                if enabled_before {
                    voice.runtime.lock().unwrap().begin_app_icon(false);
                    publish(&app);
                }
                escape = None;
                pressed_before = false;
            }
            enabled_before = enabled;
            std::thread::sleep(Duration::from_millis(16));
        }
    });
}
