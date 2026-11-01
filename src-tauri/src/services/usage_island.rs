#[cfg(any(windows, target_os = "macos"))]
use crate::services::voice_input::island_input::CursorTracker;
#[cfg(any(windows, target_os = "macos"))]
use std::sync::{Mutex, Once};
#[cfg(any(windows, target_os = "macos"))]
use std::time::Duration;
#[cfg(any(windows, target_os = "macos"))]
use tauri::Emitter;
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalRect, PhysicalSize, WebviewWindow};

pub const WINDOW_LABEL: &str = "usage-island";
const COLLAPSED_SIZE: f64 = 60.0;
// The voice island keeps the bottom of the screen, so this one starts at the top.
const TOP_GAP: f64 = 24.0;

#[cfg(any(windows, target_os = "macos"))]
const HOVER_EVENT: &str = "usage-island-hover";
// The hit area grows over IPC, so a quick sweep off the logo must not count as leaving.
#[cfg(any(windows, target_os = "macos"))]
const LEAVE_TICKS: u32 = 6;

/// The part of the window the island covers, in logical window coordinates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct HitArea {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

const LOGO_AREA: HitArea = HitArea {
    x: 0.0,
    y: 0.0,
    width: COLLAPSED_SIZE,
    height: COLLAPSED_SIZE,
};

#[cfg(any(windows, target_os = "macos"))]
static HIT_AREA: Mutex<HitArea> = Mutex::new(LOGO_AREA);

impl HitArea {
    #[cfg(any(windows, target_os = "macos", test))]
    fn contains(&self, x: f64, y: f64) -> bool {
        let (x, y) = (x - self.x, y - self.y);
        if !(0.0..self.width).contains(&x) || !(0.0..self.height).contains(&y) {
            return false;
        }
        let radius = self.width.min(self.height) / 2.0;
        let dx = x - x.clamp(radius, self.width - radius);
        let dy = y - y.clamp(radius, self.height - radius);
        dx * dx + dy * dy <= radius * radius
    }
}

// The window stays at its open size, so everything outside the island passes the
// mouse through. The page cannot see the cursor leave once it does, so hover lives here.
#[cfg(any(windows, target_os = "macos"))]
fn track_cursor(app: &AppHandle) {
    static STARTED: Once = Once::new();
    STARTED.call_once(|| {
        let app = app.clone();
        std::thread::spawn(move || {
            let mut tracker = CursorTracker::default();
            let mut hovered = false;
            let mut outside = 0;
            loop {
                let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
                    tracker = CursorTracker::default();
                    hovered = false;
                    outside = 0;
                    std::thread::sleep(Duration::from_millis(200));
                    continue;
                };
                let area = *HIT_AREA.lock().unwrap();
                let inside = tracker.update_region(&window, |x, y| area.contains(x, y));
                outside = if inside { 0 } else { outside + 1 };
                if (inside && !hovered) || (hovered && outside >= LEAVE_TICKS) {
                    hovered = inside;
                    let _ = window.emit_to(WINDOW_LABEL, HOVER_EVENT, inside);
                }
                std::thread::sleep(Duration::from_millis(16));
            }
        });
    });
}

#[cfg(target_os = "linux")]
fn apply_input_region(window: &WebviewWindow, area: HitArea) {
    use gtk::{
        cairo::{RectangleInt, Region},
        prelude::*,
    };
    let island = window.clone();
    let _ = window.run_on_main_thread(move || {
        let Ok(widget) = island.gtk_window() else {
            log::warn!("[UsageIsland] could not configure the island input region");
            return;
        };
        let region = Region::create();
        let rectangle = RectangleInt::new(
            area.x.floor() as i32,
            area.y.floor() as i32,
            area.width.ceil() as i32,
            area.height.ceil() as i32,
        );
        if region.union_rectangle(&rectangle).is_ok() {
            widget.realize();
            widget.input_shape_combine_region(Some(&region));
        }
    });
}

pub fn set_hit_area(window: &WebviewWindow, area: HitArea) {
    #[cfg(any(windows, target_os = "macos"))]
    {
        let _ = window;
        *HIT_AREA.lock().unwrap() = area;
    }
    #[cfg(target_os = "linux")]
    apply_input_region(window, area);
}

fn default_position(area: &PhysicalRect<i32, u32>, scale: f64) -> PhysicalPosition<i32> {
    PhysicalPosition::new(
        (f64::from(area.position.x) + (f64::from(area.size.width) - COLLAPSED_SIZE * scale) / 2.0)
            .round() as i32,
        (f64::from(area.position.y) + TOP_GAP * scale).round() as i32,
    )
}

fn fitted_position(
    position: PhysicalPosition<i32>,
    current: PhysicalSize<u32>,
    target: PhysicalSize<u32>,
    anchor: (f64, f64),
    area: &PhysicalRect<i32, u32>,
) -> PhysicalPosition<i32> {
    let shift = |from: u32, to: u32, part: f64| ((f64::from(from) - f64::from(to)) * part) as i32;
    let x = position.x + shift(current.width, target.width, anchor.0);
    let y = position.y + shift(current.height, target.height, anchor.1);
    let right = area.position.x + area.size.width as i32 - target.width as i32;
    let bottom = area.position.y + area.size.height as i32 - target.height as i32;
    PhysicalPosition::new(
        x.min(right).max(area.position.x),
        y.min(bottom).max(area.position.y),
    )
}

fn show(app: &AppHandle) -> Result<(), String> {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        return window.show().map_err(|error| error.to_string());
    }
    let window = tauri::WebviewWindowBuilder::new(
        app,
        WINDOW_LABEL,
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("EchoBird")
    .inner_size(COLLAPSED_SIZE, COLLAPSED_SIZE)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focusable(false)
    .focused(false)
    .visible(false)
    .build()
    .map_err(|error| error.to_string())?;
    set_hit_area(&window, LOGO_AREA);
    #[cfg(any(windows, target_os = "macos"))]
    {
        window
            .set_ignore_cursor_events(true)
            .map_err(|error| error.to_string())?;
        track_cursor(app);
    }
    if let Some(monitor) = app
        .get_webview_window("main")
        .and_then(|main| main.current_monitor().ok().flatten())
        .or_else(|| window.current_monitor().ok().flatten())
    {
        let _ = window.set_position(default_position(
            monitor.work_area(),
            monitor.scale_factor(),
        ));
    }
    Ok(())
}

pub fn set_visible(app: &AppHandle, visible: bool) -> Result<(), String> {
    if visible {
        return show(app);
    }
    match app.get_webview_window(WINDOW_LABEL) {
        Some(window) => window.destroy().map_err(|error| error.to_string()),
        None => Ok(()),
    }
}

pub fn resize(
    window: &WebviewWindow,
    width: f64,
    height: f64,
    anchor: (f64, f64),
) -> Result<(), String> {
    let scale = window.scale_factor().map_err(|error| error.to_string())?;
    let target = PhysicalSize::new(
        (width * scale).ceil() as u32,
        (height * scale).ceil() as u32,
    );
    let current = window.outer_size().map_err(|error| error.to_string())?;
    if current == target {
        return Ok(());
    }
    let position = window.outer_position().map_err(|error| error.to_string())?;
    let monitor = window
        .current_monitor()
        .map_err(|error| error.to_string())?;
    if let Some(monitor) = monitor {
        let next = fitted_position(position, current, target, anchor, monitor.work_area());
        window
            .set_position(next)
            .map_err(|error| error.to_string())?;
    }
    window.set_size(target).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area() -> PhysicalRect<i32, u32> {
        PhysicalRect {
            position: PhysicalPosition::new(-1920, 40),
            size: PhysicalSize::new(1920, 1000),
        }
    }

    #[test]
    fn only_the_visible_island_captures_the_mouse() {
        let logo = HitArea {
            x: 276.0,
            ..LOGO_AREA
        };
        assert!(logo.contains(306.0, 30.0));
        assert!(!logo.contains(278.0, 2.0));
        assert!(!logo.contains(100.0, 30.0));
        let open = HitArea {
            x: 0.0,
            y: 0.0,
            width: 336.0,
            height: 60.0,
        };
        assert!(open.contains(100.0, 30.0));
        assert!(open.contains(30.0, 2.0));
        assert!(!open.contains(2.0, 2.0));
        assert!(!open.contains(335.0, 59.0));
        let dial = HitArea {
            x: 0.0,
            y: 0.0,
            width: 216.0,
            height: 216.0,
        };
        assert!(dial.contains(108.0, 2.0));
        assert!(!dial.contains(4.0, 4.0));
    }

    #[test]
    fn starts_at_the_top_center_of_the_work_area_at_any_scale() {
        assert_eq!(
            default_position(&area(), 1.0),
            PhysicalPosition::new(-990, 64)
        );
        assert_eq!(
            default_position(&area(), 2.0),
            PhysicalPosition::new(-1020, 88)
        );
    }

    #[test]
    fn side_expansion_keeps_the_logo_in_place_while_it_fits() {
        assert_eq!(
            fitted_position(
                PhysicalPosition::new(-1000, 100),
                PhysicalSize::new(60, 60),
                PhysicalSize::new(400, 60),
                (0.0, 0.0),
                &area(),
            ),
            PhysicalPosition::new(-1000, 100)
        );
    }

    #[test]
    fn reversed_expansion_keeps_the_logo_at_the_far_edge() {
        assert_eq!(
            fitted_position(
                PhysicalPosition::new(-1000, 100),
                PhysicalSize::new(60, 60),
                PhysicalSize::new(400, 60),
                (1.0, 0.0),
                &area(),
            ),
            PhysicalPosition::new(-1340, 100)
        );
        assert_eq!(
            fitted_position(
                PhysicalPosition::new(-1000, 500),
                PhysicalSize::new(60, 60),
                PhysicalSize::new(60, 254),
                (0.0, 1.0),
                &area(),
            ),
            PhysicalPosition::new(-1000, 306)
        );
    }

    #[test]
    fn expansion_never_leaves_the_work_area() {
        assert_eq!(
            fitted_position(
                PhysicalPosition::new(-100, 1000),
                PhysicalSize::new(60, 60),
                PhysicalSize::new(400, 60),
                (0.0, 0.0),
                &area(),
            ),
            PhysicalPosition::new(-400, 980)
        );
    }

    #[test]
    fn the_dial_grows_around_its_center_and_shrinks_back_to_it() {
        let opened = fitted_position(
            PhysicalPosition::new(-1000, 300),
            PhysicalSize::new(60, 60),
            PhysicalSize::new(216, 216),
            (0.5, 0.5),
            &area(),
        );
        assert_eq!(opened, PhysicalPosition::new(-1078, 222));
        assert_eq!(
            fitted_position(
                opened,
                PhysicalSize::new(216, 216),
                PhysicalSize::new(60, 60),
                (0.5, 0.5),
                &area(),
            ),
            PhysicalPosition::new(-1000, 300)
        );
    }
}
