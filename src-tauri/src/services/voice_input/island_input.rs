pub(super) const BODY_WIDTH: f64 = 176.0;
pub(super) const BODY_HEIGHT: f64 = 44.0;
pub(super) const SHADOW_PADDING: f64 = 24.0;
// Room above the capsule for the live transcript bubble while speaking.
pub(super) const BUBBLE_WIDTH: f64 = 280.0;
pub(super) const BUBBLE_HEIGHT: f64 = 104.0;
const BUBBLE_GAP: f64 = 10.0;
pub(super) const WINDOW_WIDTH: f64 = BUBBLE_WIDTH + SHADOW_PADDING * 2.0;
pub(super) const BODY_LEFT: f64 = (WINDOW_WIDTH - BODY_WIDTH) / 2.0;
pub(super) const BODY_TOP: f64 = SHADOW_PADDING + BUBBLE_HEIGHT + BUBBLE_GAP;
pub(super) const WINDOW_HEIGHT: f64 = BODY_TOP + BODY_HEIGHT + SHADOW_PADDING;
// Keep the visible capsule's existing 56px bottom inset after shrinking its host.
pub(super) const BOTTOM_GAP: f64 = 56.0 - SHADOW_PADDING;

fn contains(x: f64, y: f64) -> bool {
    let x = x - BODY_LEFT;
    let y = y - BODY_TOP;
    if !(0.0..BODY_WIDTH).contains(&x) || !(0.0..BODY_HEIGHT).contains(&y) {
        return false;
    }
    let radius = BODY_HEIGHT / 2.0;
    let dx = x - x.clamp(radius, BODY_WIDTH - radius);
    let dy = y - radius;
    dx * dx + dy * dy <= radius * radius
}

#[cfg(any(windows, target_os = "macos", test))]
#[derive(Default)]
pub(crate) struct CursorTracker {
    ignored: Option<bool>,
}

#[cfg(any(windows, target_os = "macos", test))]
impl CursorTracker {
    fn desired_ignore(&self, inside: bool, pressed: bool) -> bool {
        !(inside || (pressed && self.ignored == Some(false)))
    }

    #[cfg(any(windows, target_os = "macos"))]
    pub(super) fn update(&mut self, window: &tauri::WebviewWindow, zoom: f64) {
        self.update_region(window, |x, y| contains(x / zoom, y / zoom));
    }

    /// Lets the mouse through outside `region`, given in logical window coordinates.
    #[cfg(any(windows, target_os = "macos"))]
    pub(crate) fn update_region(
        &mut self,
        window: &tauri::WebviewWindow,
        region: impl Fn(f64, f64) -> bool,
    ) -> bool {
        let inside = window
            .cursor_position()
            .and_then(|cursor| {
                let origin = window.inner_position()?;
                let scale = window.scale_factor()?;
                Ok(region(
                    (cursor.x - f64::from(origin.x)) / scale,
                    (cursor.y - f64::from(origin.y)) / scale,
                ))
            })
            .unwrap_or(true);
        let ignore = self.desired_ignore(inside, super::native::left_mouse_down());
        if self.ignored != Some(ignore) && window.set_ignore_cursor_events(ignore).is_ok() {
            self.ignored = Some(ignore);
        }
        inside
    }
}

#[cfg(windows)]
pub(super) fn configure(window: &tauri::WebviewWindow, _zoom: f64) -> tauri::Result<()> {
    window.set_ignore_cursor_events(true)
}

/// A click on an ordinary macOS window activates the whole app, which raises the
/// main window and takes focus from the app being dictated into; `focusable(false)`
/// only stops the island itself from becoming key, so it also opts out of activation.
#[cfg(target_os = "macos")]
pub(super) fn configure(window: &tauri::WebviewWindow, _zoom: f64) -> tauri::Result<()> {
    use objc2::{msg_send, runtime::AnyObject, runtime::Bool, sel};
    window.set_ignore_cursor_events(true)?;
    let island = window.ns_window()? as usize;
    window.run_on_main_thread(move || {
        // SAFETY: the pointer is the live NSWindow of this webview window, messaged on the main thread.
        let island = unsafe { &*(island as *const AnyObject) };
        let supported: Bool =
            unsafe { msg_send![island, respondsToSelector: sel!(_setPreventsActivation:)] };
        if supported.as_bool() {
            let _: () = unsafe { msg_send![island, _setPreventsActivation: Bool::YES] };
        } else {
            log::warn!("[VoiceInput] the island cannot opt out of app activation");
        }
    })
}

/// Rows of the capsule's input mask in logical window pixels at `zoom`.
#[cfg(any(target_os = "linux", test))]
fn input_rows(zoom: f64) -> Vec<(i32, i32, i32)> {
    let inside =
        |x: i32, y: i32| contains((f64::from(x) + 0.5) / zoom, (f64::from(y) + 0.5) / zoom);
    (0..(WINDOW_HEIGHT * zoom).ceil() as i32)
        .filter_map(|y| {
            let mut columns = (0..(WINDOW_WIDTH * zoom).ceil() as i32).filter(|&x| inside(x, y));
            let left = columns.next()?;
            let right = columns.next_back().unwrap_or(left);
            Some((left, y, right - left + 1))
        })
        .collect()
}

#[cfg(target_os = "linux")]
pub(super) fn configure(window: &tauri::WebviewWindow, zoom: f64) -> tauri::Result<()> {
    use gtk::{
        cairo::{RectangleInt, Region},
        prelude::*,
    };
    let island = window.clone();
    window.run_on_main_thread(move || {
        let Ok(widget) = island.gtk_window() else {
            log::warn!("[VoiceInput] could not configure the island input region");
            return;
        };
        let region = Region::create();
        for (x, y, width) in input_rows(zoom) {
            if let Err(error) = region.union_rectangle(&RectangleInt::new(x, y, width, 1)) {
                log::warn!("[VoiceInput] could not create the island input region: {error}");
                return;
            }
        }
        widget.realize();
        widget.input_shape_combine_region(Some(&region));
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_visible_capsule_captures_the_mouse_at_any_scale_or_monitor_origin() {
        for (x, y, expected) in [
            (112.0, 46.0, true),
            (48.0, 46.0, true),
            (176.0, 46.0, true),
            (24.0, 46.0, true),
            (24.0, 24.0, false),
            (199.0, 25.0, false),
            (112.0, 23.0, false),
            (112.0, 68.0, false),
            (201.0, 46.0, false),
            (0.0, 0.0, false),
            (223.0, 91.0, false),
        ] {
            // Points are given relative to the capsule's shadow box.
            let (x, y) = (
                x + BODY_LEFT - SHADOW_PADDING,
                y + BODY_TOP - SHADOW_PADDING,
            );
            for (origin_x, origin_y, scale) in [
                (-1920.0, -400.0, 1.0),
                (50.0, 80.0, 1.25),
                (2560.0, 0.0, 2.0),
            ] {
                let cursor_x = origin_x + x * scale;
                let cursor_y = origin_y + y * scale;
                assert_eq!(
                    contains((cursor_x - origin_x) / scale, (cursor_y - origin_y) / scale),
                    expected
                );
            }
        }
    }

    #[test]
    fn dragging_keeps_mouse_capture_until_release_and_outside_clicks_do_not_enable_it() {
        let mut tracker = CursorTracker::default();
        assert!(tracker.desired_ignore(false, false));
        tracker.ignored = Some(true);
        assert!(tracker.desired_ignore(false, true));
        assert!(!tracker.desired_ignore(true, false));
        tracker.ignored = Some(false);
        assert!(!tracker.desired_ignore(false, true));
        assert!(tracker.desired_ignore(false, false));
    }

    #[test]
    fn linux_input_mask_matches_the_visible_capsule_without_its_shadow_or_corners() {
        for zoom in [0.7, 1.0, 1.5] {
            let rows = input_rows(zoom);
            assert!(rows.len().abs_diff((BODY_HEIGHT * zoom).round() as usize) <= 1);
            for y in 0..(WINDOW_HEIGHT * zoom) as i32 {
                for x in 0..(WINDOW_WIDTH * zoom) as i32 {
                    let masked = rows
                        .iter()
                        .any(|&(left, top, width)| y == top && x >= left && x < left + width);
                    let inside = contains((f64::from(x) + 0.5) / zoom, (f64::from(y) + 0.5) / zoom);
                    assert_eq!(masked, inside, "zoom={zoom} x={x} y={y}");
                }
            }
        }
    }
}
