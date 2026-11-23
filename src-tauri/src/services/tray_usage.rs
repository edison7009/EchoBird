//! Read-only usage preview anchored to the system tray.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::time::Duration;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalRect, PhysicalSize, Rect, WebviewWindow,
};

pub const WINDOW_LABEL: &str = "tray-usage";
const STATE_EVENT: &str = "tray-usage-state";
const REFRESH_EVENT: &str = "usage-island-tick";
const HOVER_DELAY: Duration = Duration::from_millis(240);
const MAX_ITEMS: usize = 6;
const CONTENT_WIDTH: f64 = 280.0;
const ROW_HEIGHT: f64 = 52.0;
const ROW_GAP: f64 = 6.0;
const CONTENT_VERTICAL_PADDING: f64 = 24.0;
const SHADOW_MARGIN: f64 = 12.0;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Theme {
    Auto,
    Dark,
    Light,
    System,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    theme: Theme,
    #[serde(skip_serializing_if = "Option::is_none")]
    background: Option<String>,
    #[serde(default = "default_percentage")]
    opacity: u8,
    #[serde(default = "default_percentage")]
    scale: u8,
    #[serde(default)]
    refresh_seconds: u64,
    items: Vec<serde_json::Value>,
}

fn default_percentage() -> u8 {
    100
}

#[derive(Clone, Serialize)]
pub struct Snapshot {
    revision: u64,
    preview: Option<Preview>,
}

#[derive(Clone, Copy)]
struct Hover {
    anchor: PhysicalRect<f64, f64>,
    phase: HoverPhase,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum HoverPhase {
    Pending,
    Visible,
    Dismissed,
}

#[derive(Default)]
struct State {
    preview: Option<Preview>,
    hover: Option<Hover>,
    hover_generation: u64,
    revision: u64,
}

impl State {
    fn visible_hover(&self) -> Option<Hover> {
        self.hover
            .filter(|hover| hover.phase == HoverPhase::Visible)
    }

    fn snapshot(&self) -> Option<Snapshot> {
        self.visible_hover()?;
        Some(Snapshot {
            revision: self.revision,
            preview: Some(self.preview.clone()?),
        })
    }

    fn enter(&mut self, anchor: PhysicalRect<f64, f64>, recover: bool) -> Option<u64> {
        if recover && self.hover.is_some() {
            return None;
        }
        self.revision += 1;
        self.hover_generation += 1;
        self.hover = Some(Hover {
            anchor,
            phase: HoverPhase::Pending,
        });
        Some(self.hover_generation)
    }

    fn complete_delay(&mut self, generation: u64) -> bool {
        if self.hover_generation != generation {
            return false;
        }
        let Some(hover) = &mut self.hover else {
            return false;
        };
        if hover.phase != HoverPhase::Pending {
            return false;
        }
        hover.phase = HoverPhase::Visible;
        self.revision += 1;
        self.preview.is_some()
    }

    fn dismiss(&mut self) {
        if let Some(hover) = &mut self.hover {
            hover.phase = HoverPhase::Dismissed;
            self.revision += 1;
        }
    }

    fn leave(&mut self) {
        self.revision += 1;
        self.hover_generation += 1;
        self.hover = None;
    }
}

static STATE: Mutex<State> = Mutex::new(State {
    preview: None,
    hover: None,
    hover_generation: 0,
    revision: 0,
});
static CREATE: Mutex<()> = Mutex::new(());
static REFRESH: Mutex<Option<(Duration, tauri::async_runtime::JoinHandle<()>)>> = Mutex::new(None);

pub fn snapshot() -> Option<Snapshot> {
    STATE.lock().unwrap().snapshot()
}

fn window(app: &AppHandle) -> Result<WebviewWindow, String> {
    let _guard = CREATE.lock().unwrap();
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        return Ok(window);
    }
    let preview = tauri::WebviewWindowBuilder::new(
        app,
        WINDOW_LABEL,
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("EchoBird")
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focusable(false)
    .focused(false)
    .visible(false)
    .inner_size(CONTENT_WIDTH + 2.0 * SHADOW_MARGIN, 100.0)
    .build()
    .map_err(|error| error.to_string())?;
    if let Err(error) = preview.set_ignore_cursor_events(true) {
        let _ = preview.destroy();
        return Err(error.to_string());
    }
    Ok(preview)
}

fn publish(app: &AppHandle) -> Result<(), String> {
    if snapshot().is_none() {
        return Ok(());
    }
    let window = window(app)?;
    if let Some(current) = snapshot() {
        window
            .emit_to(WINDOW_LABEL, STATE_EVENT, current)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

pub fn update(app: &AppHandle, preview: Option<Preview>) -> Result<(), String> {
    if preview
        .as_ref()
        .is_some_and(|preview| preview.items.len() > MAX_ITEMS)
    {
        return Err("Too many tray usage sources".into());
    }
    let enabled = preview.is_some();
    let refresh = preview
        .as_ref()
        .filter(|preview| !preview.items.is_empty() && preview.refresh_seconds > 0)
        .map(|preview| Duration::from_secs(preview.refresh_seconds.clamp(300, 3600)));
    configure_refresh(app, refresh);
    let (available_changed, show) = {
        let mut state = STATE.lock().unwrap();
        let available_changed = state.preview.is_some() != enabled;
        state.preview = preview;
        state.revision += 1;
        (
            available_changed,
            enabled && state.visible_hover().is_some(),
        )
    };
    if available_changed {
        if let Some(tray) = app.tray_by_id("main-tray") {
            // The list, including its empty state, takes the place of the system tooltip.
            let _ = tray.set_tooltip((!enabled).then_some("EchoBird"));
        }
    }
    if show {
        publish(app)?;
    } else if !enabled {
        hide_window(app);
    }
    Ok(())
}

fn configure_refresh(app: &AppHandle, interval: Option<Duration>) {
    let mut refresh = REFRESH.lock().unwrap();
    if refresh.as_ref().map(|(period, _)| *period) == interval {
        return;
    }
    if let Some((_, task)) = refresh.take() {
        task.abort();
    }
    if let Some(period) = interval {
        let app = app.clone();
        let task = tauri::async_runtime::spawn(async move {
            let mut timer = tokio::time::interval_at(tokio::time::Instant::now() + period, period);
            timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                timer.tick().await;
                let _ = app.emit_to("main", REFRESH_EVENT, ());
            }
        });
        *refresh = Some((period, task));
    }
}

pub fn enter(app: &AppHandle, rect: Rect) {
    start_hover(app, rect, false);
}

#[cfg(windows)]
pub fn pointer_moved(app: &AppHandle, rect: Rect) {
    start_hover(app, rect, true);
}

fn start_hover(app: &AppHandle, rect: Rect, recover: bool) {
    let position = rect.position.to_physical::<f64>(1.0);
    let size = rect.size.to_physical::<f64>(1.0);
    if size.width <= 0.0 || size.height <= 0.0 {
        return;
    }
    let anchor = PhysicalRect { position, size };
    let Some(generation) = STATE.lock().unwrap().enter(anchor, recover) else {
        return;
    };
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(HOVER_DELAY).await;
        #[cfg(windows)]
        if !cursor_still_over_icon(&app, anchor, generation) {
            return;
        }
        let show = STATE.lock().unwrap().complete_delay(generation);
        if show {
            let _ = publish(&app);
        }
        // Windows may skip Leave when its tray event timer last saw the cursor
        // inside the icon. Recover only while tracking this hover.
        #[cfg(windows)]
        loop {
            tokio::time::sleep(Duration::from_millis(80)).await;
            if !cursor_still_over_icon(&app, anchor, generation) {
                break;
            }
        }
    });
}

#[cfg(windows)]
fn cursor_still_over_icon(
    app: &AppHandle,
    anchor: PhysicalRect<f64, f64>,
    generation: u64,
) -> bool {
    let over_icon = app
        .cursor_position()
        .is_ok_and(|cursor| anchor_contains(anchor, cursor));
    let mut state = STATE.lock().unwrap();
    if state.hover_generation != generation {
        return false;
    }
    if over_icon {
        return true;
    }
    state.leave();
    drop(state);
    hide_window(app);
    false
}

#[cfg(any(windows, test))]
fn anchor_contains(anchor: PhysicalRect<f64, f64>, cursor: PhysicalPosition<f64>) -> bool {
    (anchor.position.x..anchor.position.x + anchor.size.width).contains(&cursor.x)
        && (anchor.position.y..anchor.position.y + anchor.size.height).contains(&cursor.y)
}

fn hide_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let handle = window.clone();
        let _ = window.run_on_main_thread(move || {
            let revision = {
                let state = STATE.lock().unwrap();
                if state.visible_hover().is_some() && state.preview.is_some() {
                    return;
                }
                state.revision
            };
            let _ = handle.hide();
            let _ = handle.emit_to(
                WINDOW_LABEL,
                STATE_EVENT,
                Snapshot {
                    revision,
                    preview: None,
                },
            );
        });
    }
}

pub fn leave(app: &AppHandle) {
    STATE.lock().unwrap().leave();
    hide_window(app);
}

pub fn dismiss(app: &AppHandle) {
    STATE.lock().unwrap().dismiss();
    hide_window(app);
}

fn geometry(
    anchor: PhysicalRect<f64, f64>,
    count: usize,
    work_area: PhysicalRect<i32, u32>,
    display_scale: f64,
    requested_zoom: f64,
) -> (PhysicalPosition<i32>, PhysicalSize<u32>, f64) {
    let rows = count.max(1); // The empty tip takes up one row.
    let width = (CONTENT_WIDTH + 2.0 * SHADOW_MARGIN) * display_scale;
    let height = (CONTENT_VERTICAL_PADDING
        + rows as f64 * ROW_HEIGHT
        + (rows - 1) as f64 * ROW_GAP
        + 2.0 * SHADOW_MARGIN)
        * display_scale;
    let work_width = f64::from(work_area.size.width);
    let work_height = f64::from(work_area.size.height);
    let zoom = requested_zoom
        .min(work_width / width)
        .min(work_height / height);
    let width = (width * zoom).ceil().min(work_width);
    let height = (height * zoom).ceil().min(work_height);
    let left = f64::from(work_area.position.x);
    let top = f64::from(work_area.position.y);
    let bottom = top + f64::from(work_area.size.height);
    let center = anchor.position.x + anchor.size.width / 2.0;
    let x = (center - width / 2.0).clamp(left, left + f64::from(work_area.size.width) - width);
    let below = anchor.position.y + anchor.size.height;
    let above = anchor.position.y - height;
    let y = if below + height <= bottom || above < top {
        below
    } else {
        above
    }
    .clamp(top, bottom - height);
    (
        PhysicalPosition::new(x.round() as i32, y.round() as i32),
        PhysicalSize::new(width as u32, height as u32),
        zoom,
    )
}

pub fn ready(window: WebviewWindow, revision: u64) -> Result<(), String> {
    let handle = window.clone();
    window
        .run_on_main_thread(move || {
            let (anchor, count, zoom) = {
                let state = STATE.lock().unwrap();
                if state.revision != revision {
                    return;
                }
                let (Some(hover), Some(preview)) = (state.visible_hover(), &state.preview) else {
                    return;
                };
                (
                    hover.anchor,
                    preview.items.len(),
                    f64::from(preview.scale.clamp(70, 150)) / 100.0,
                )
            };
            let app = handle.app_handle();
            let monitor = app
                .monitor_from_point(
                    anchor.position.x + anchor.size.width / 2.0,
                    anchor.position.y + anchor.size.height / 2.0,
                )
                .ok()
                .flatten()
                .or_else(|| app.primary_monitor().ok().flatten());
            let Some(monitor) = monitor else {
                return;
            };
            let (position, size, zoom) = geometry(
                anchor,
                count,
                *monitor.work_area(),
                monitor.scale_factor(),
                zoom,
            );
            if handle.set_zoom(zoom).is_ok()
                && handle.set_size(size).is_ok()
                && handle.set_position(position).is_ok()
            {
                let _ = handle.show();
            }
        })
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn anchor(x: f64, y: f64) -> PhysicalRect<f64, f64> {
        PhysicalRect {
            position: PhysicalPosition::new(x, y),
            size: PhysicalSize::new(40.0, 28.0),
        }
    }

    #[test]
    fn preview_stays_on_its_screen_and_opens_inside_the_work_area() {
        let work_area = PhysicalRect {
            position: PhysicalPosition::new(-1920, 25),
            size: PhysicalSize::new(1920, 1015),
        };
        let (top, size, _) = geometry(anchor(-46.0, 0.0), 2, work_area, 1.0, 1.0);
        assert_eq!(top, PhysicalPosition::new(-304, 28));
        assert_eq!(size, PhysicalSize::new(304, 158));
        let (bottom, size, _) = geometry(anchor(-1900.0, 1040.0), 2, work_area, 1.0, 1.0);
        assert_eq!(
            bottom,
            PhysicalPosition::new(-1920, 1040 - size.height as i32)
        );
        let (_, hidpi, _) = geometry(anchor(-900.0, 0.0), 2, work_area, 2.0, 1.0);
        assert_eq!(hidpi, PhysicalSize::new(608, 316));
        let (_, empty, zoom) = geometry(anchor(-900.0, 0.0), 0, work_area, 2.0, 1.2);
        assert_eq!(empty, PhysicalSize::new(730, 240));
        assert_eq!(zoom, 1.2);
    }

    #[test]
    fn leaving_or_reentering_invalidates_the_previous_hover() {
        let mut state = State {
            preview: Some(Preview {
                theme: Theme::Dark,
                background: None,
                opacity: 100,
                scale: 100,
                refresh_seconds: 0,
                items: vec![],
            }),
            ..State::default()
        };
        let first = state.enter(anchor(0.0, 0.0), false).unwrap();
        assert!(state.snapshot().is_none());
        state.leave();
        assert!(!state.complete_delay(first));
        let second = state.enter(anchor(0.0, 0.0), false).unwrap();
        assert!(!state.complete_delay(first));
        let pending_revision = state.revision;
        assert!(state.complete_delay(second));
        let revision = state.snapshot().unwrap().revision;
        assert!(revision > pending_revision);
        state.leave();
        assert!(state.snapshot().is_none());
        assert!(state.revision > revision);
    }

    #[test]
    fn recovering_a_missed_enter_does_not_reopen_a_dismissed_hover() {
        let rect = anchor(-40.0, 0.0);
        assert!(anchor_contains(rect, PhysicalPosition::new(-20.0, 14.0)));
        assert!(!anchor_contains(rect, PhysicalPosition::new(0.0, 14.0)));
        assert!(!anchor_contains(rect, PhysicalPosition::new(-20.0, 28.0)));

        let mut state = State::default();
        let generation = state.enter(rect, true).unwrap();
        state.dismiss();
        assert!(!state.complete_delay(generation));
        assert!(state.enter(rect, true).is_none());
        state.leave();
        assert!(state.enter(rect, true).is_some());
    }
}
