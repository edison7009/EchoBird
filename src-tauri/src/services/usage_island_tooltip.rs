use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalRect, PhysicalSize, WebviewWindow,
};

pub const WINDOW_LABEL: &str = "usage-island-tooltip";
const EVENT: &str = "usage-island-tooltip-state";
static REVISION: AtomicU64 = AtomicU64::new(0);
static CREATE: Mutex<()> = Mutex::new(());
static SNAPSHOT: Mutex<Option<Pending>> = Mutex::new(None);

#[derive(Clone, Deserialize, Serialize)]
pub struct Anchor {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    item: serde_json::Value,
    theme: String,
    reset_label: String,
    anchor: Anchor,
    width: f64,
    height: f64,
    zoom: f64,
    #[serde(default)]
    side: bool,
    #[serde(default)]
    clearance: Option<Anchor>,
    #[serde(default)]
    direction: Option<String>,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    request: Request,
    placement: &'static str,
    arrow_x: f64,
    revision: u64,
}

#[derive(Clone)]
struct Pending {
    snapshot: Snapshot,
    origin: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
}

pub fn snapshot() -> Option<Snapshot> {
    SNAPSHOT
        .lock()
        .unwrap()
        .as_ref()
        .map(|state| state.snapshot.clone())
}

pub fn ready(app: &AppHandle, revision: u64) -> Result<(), String> {
    log::debug!("[UsageIsland] detail ready");
    let state = SNAPSHOT
        .lock()
        .unwrap()
        .as_ref()
        .filter(|state| state.snapshot.revision == revision)
        .cloned();
    let Some(state) = state else {
        return Ok(());
    };
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return Ok(());
    };
    let handle = window.clone();
    let app = app.clone();
    window
        .run_on_main_thread(move || {
            if REVISION.load(Ordering::SeqCst) != revision
                || app
                    .get_webview_window(super::usage_island::WINDOW_LABEL)
                    .is_none()
            {
                return;
            }
            if handle.set_size(state.size).is_ok() && handle.set_position(state.origin).is_ok() {
                let _ = handle.show();
            }
        })
        .map_err(|error| error.to_string())
}

pub fn hide(app: &AppHandle) {
    let revision = REVISION.fetch_add(1, Ordering::SeqCst) + 1;
    {
        let mut state = SNAPSHOT.lock().unwrap();
        if REVISION.load(Ordering::SeqCst) == revision {
            *state = None;
        }
    }
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let handle = window.clone();
        let _ = window.run_on_main_thread(move || {
            if REVISION.load(Ordering::SeqCst) == revision {
                let _ = handle.hide();
            }
        });
    }
}

pub fn close(app: &AppHandle) {
    hide(app);
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let _ = window.destroy();
    }
}

fn valid(request: &Request) -> bool {
    [
        request.anchor.x,
        request.anchor.y,
        request.anchor.width,
        request.anchor.height,
        request.width,
        request.height,
        request.zoom,
    ]
    .iter()
    .all(|value| value.is_finite())
        && request.anchor.width > 0.0
        && request.anchor.height > 0.0
        && (1.0..=600.0).contains(&request.width)
        && (1.0..=600.0).contains(&request.height)
        && (0.5..=2.0).contains(&request.zoom)
        && ["dark", "light"].contains(&request.theme.as_str())
        && request.clearance.as_ref().is_none_or(|bounds| {
            [bounds.x, bounds.y, bounds.width, bounds.height]
                .iter()
                .all(|value| value.is_finite())
                && bounds.width > 0.0
                && bounds.height > 0.0
        })
}

// Content coordinates are physical pixels; the auxiliary window adds a transparent
// shadow margin. Nothing resizes or repositions the parent island on hover.
fn position(
    anchor: PhysicalRect<f64, f64>,
    size: PhysicalSize<f64>,
    area: PhysicalRect<i32, u32>,
    scale: f64,
) -> (PhysicalPosition<i32>, &'static str, f64) {
    let padding = 12.0 * scale;
    let gap = 10.0 * scale;
    let min_x = f64::from(area.position.x) + padding;
    let max_x =
        (f64::from(area.position.x) + f64::from(area.size.width) - size.width - padding).max(min_x);
    let min_y = f64::from(area.position.y) + padding;
    let max_y = (f64::from(area.position.y) + f64::from(area.size.height) - size.height - padding)
        .max(min_y);
    let center = anchor.position.x + anchor.size.width / 2.0;
    let left = (center - size.width / 2.0).clamp(min_x, max_x);
    let above = anchor.position.y - size.height - gap >= min_y;
    let top = if above {
        anchor.position.y - size.height - gap
    } else {
        anchor.position.y + anchor.size.height + gap
    }
    .clamp(min_y, max_y);
    let arrow = ((center - left) / scale).clamp(18.0, (size.width / scale - 18.0).max(18.0));
    (
        PhysicalPosition::new(
            (left - padding).floor() as i32,
            (top - padding).floor() as i32,
        ),
        if above { "above" } else { "below" },
        arrow,
    )
}

fn create(app: &AppHandle) -> Result<WebviewWindow, String> {
    let _creation = CREATE.lock().unwrap();
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        return Ok(window);
    }
    let result = tauri::WebviewWindowBuilder::new(
        app,
        WINDOW_LABEL,
        tauri::WebviewUrl::App("index.html".into()),
    )
    .title("EchoBird")
    .inner_size(300.0, 240.0)
    .decorations(false)
    .transparent(true)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focusable(false)
    .focused(false)
    .visible(false)
    .build();
    // Rapidly changing sources can race the first creation. Reuse its one window.
    let window = result
        .or_else(|error| app.get_webview_window(WINDOW_LABEL).ok_or(error))
        .map_err(|error| error.to_string())?;
    #[cfg(not(target_os = "linux"))]
    window
        .set_ignore_cursor_events(true)
        .map_err(|error| error.to_string())?;
    #[cfg(target_os = "linux")]
    {
        use gtk::{cairo::Region, prelude::*};
        let handle = window.clone();
        window
            .run_on_main_thread(move || {
                if let Ok(widget) = handle.gtk_window() {
                    widget.realize();
                    widget.input_shape_combine_region(Some(&Region::create()));
                }
            })
            .map_err(|error| error.to_string())?;
    }
    Ok(window)
}

pub fn show(
    app: &AppHandle,
    source: &WebviewWindow,
    request: Option<Request>,
) -> Result<(), String> {
    let Some(request) = request else {
        hide(app);
        return Ok(());
    };
    if !valid(&request) {
        return Err("Invalid usage tooltip".into());
    }
    log::debug!("[UsageIsland] detail requested");
    let revision = REVISION.fetch_add(1, Ordering::SeqCst) + 1;
    if let Some(window) = app.get_webview_window(WINDOW_LABEL) {
        let handle = window.clone();
        let _ = window.run_on_main_thread(move || {
            if REVISION.load(Ordering::SeqCst) == revision {
                let _ = handle.hide();
            }
        });
    }
    let origin = source.inner_position().map_err(|error| error.to_string())?;
    let source_scale = source.scale_factor().map_err(|error| error.to_string())? * request.zoom;
    let anchor = PhysicalRect {
        position: PhysicalPosition::new(
            f64::from(origin.x) + request.anchor.x * source_scale,
            f64::from(origin.y) + request.anchor.y * source_scale,
        ),
        size: PhysicalSize::new(
            request.anchor.width * source_scale,
            request.anchor.height * source_scale,
        ),
    };
    let monitor = source
        .available_monitors()
        .map_err(|error| error.to_string())?
        .into_iter()
        .find(|monitor| {
            let area = monitor.work_area();
            let x = anchor.position.x + anchor.size.width / 2.0;
            let y = anchor.position.y + anchor.size.height / 2.0;
            x >= f64::from(area.position.x)
                && x < f64::from(area.position.x) + f64::from(area.size.width)
                && y >= f64::from(area.position.y)
                && y < f64::from(area.position.y) + f64::from(area.size.height)
        })
        .or_else(|| source.current_monitor().ok().flatten())
        .ok_or("No usage island monitor")?;
    let scale = monitor.scale_factor() * request.zoom;
    let size = PhysicalSize::new(request.width * scale, request.height * scale);
    let (mut origin, mut placement, mut arrow_x) =
        position(anchor, size, *monitor.work_area(), scale);
    if request.side {
        let area = monitor.work_area();
        let padding = 12.0 * scale;
        let gap = 10.0 * scale;
        let left = anchor.position.x - size.width - gap;
        let on_left = left >= f64::from(area.position.x) + padding;
        let x = if on_left {
            left
        } else {
            anchor.position.x + anchor.size.width + gap
        };
        let x = x.clamp(
            f64::from(area.position.x) + padding,
            (f64::from(area.position.x) + f64::from(area.size.width) - size.width - padding)
                .max(f64::from(area.position.x) + padding),
        );
        let center = anchor.position.y + anchor.size.height / 2.0;
        let y = (center - size.height / 2.0).clamp(
            f64::from(area.position.y) + padding,
            (f64::from(area.position.y) + f64::from(area.size.height) - size.height - padding)
                .max(f64::from(area.position.y) + padding),
        );
        origin = PhysicalPosition::new((x - padding).floor() as i32, (y - padding).floor() as i32);
        placement = if on_left { "left" } else { "right" };
        arrow_x = ((center - y) / scale).clamp(18.0, (request.height - 18.0).max(18.0));
    }
    if let (Some(bounds), Some(direction)) = (&request.clearance, &request.direction) {
        let area = monitor.work_area();
        let padding = 12.0 * scale;
        let gap = 10.0 * scale;
        let bounds = PhysicalRect {
            position: PhysicalPosition::new(
                f64::from(
                    source
                        .inner_position()
                        .map_err(|error| error.to_string())?
                        .x,
                ) + bounds.x * source_scale,
                f64::from(
                    source
                        .inner_position()
                        .map_err(|error| error.to_string())?
                        .y,
                ) + bounds.y * source_scale,
            ),
            size: PhysicalSize::new(bounds.width * source_scale, bounds.height * source_scale),
        };
        let center_x = anchor.position.x + anchor.size.width / 2.0;
        let center_y = anchor.position.y + anchor.size.height / 2.0;
        let min_x = f64::from(area.position.x) + padding;
        let min_y = f64::from(area.position.y) + padding;
        let max_x =
            (f64::from(area.position.x) + f64::from(area.size.width) - size.width - padding)
                .max(min_x);
        let max_y =
            (f64::from(area.position.y) + f64::from(area.size.height) - size.height - padding)
                .max(min_y);
        let candidates = [direction.as_str(), "above", "below", "left", "right"];
        let mut selected = None;
        for candidate in candidates {
            let mut x = (center_x - size.width / 2.0).clamp(min_x, max_x);
            let mut y = (center_y - size.height / 2.0).clamp(min_y, max_y);
            match candidate {
                "above" => y = bounds.position.y - size.height - gap,
                "below" => y = bounds.position.y + bounds.size.height + gap,
                "left" => x = bounds.position.x - size.width - gap,
                "right" => x = bounds.position.x + bounds.size.width + gap,
                _ => continue,
            }
            if x < min_x || y < min_y || x > max_x || y > max_y {
                continue;
            }
            let side = matches!(candidate, "left" | "right");
            let arrow = if side {
                (center_y - y) / scale
            } else {
                (center_x - x) / scale
            };
            let limit = if side { request.height } else { request.width };
            let direction = match candidate {
                "above" => "above",
                "below" => "below",
                "left" => "left",
                _ => "right",
            };
            selected = Some((
                PhysicalPosition::new((x - padding).floor() as i32, (y - padding).floor() as i32),
                direction,
                arrow.clamp(18.0, (limit - 18.0).max(18.0)),
            ));
            break;
        }
        let Some((position, direction, arrow)) = selected else {
            hide(app);
            return Ok(());
        };
        origin = position;
        placement = direction;
        arrow_x = arrow;
    }
    let snapshot = Snapshot {
        request,
        placement,
        arrow_x,
        revision,
    };
    if REVISION.load(Ordering::SeqCst) != revision {
        return Ok(());
    }
    {
        let mut state = SNAPSHOT.lock().unwrap();
        if REVISION.load(Ordering::SeqCst) != revision {
            return Ok(());
        }
        *state = Some(Pending {
            snapshot: snapshot.clone(),
            origin,
            size: PhysicalSize::new(
                (size.width + 24.0 * scale).ceil() as u32,
                (size.height + 24.0 * scale).ceil() as u32,
            ),
        });
    }
    let window = create(app)?;
    let handle = window.clone();
    let app = app.clone();
    window
        .run_on_main_thread(move || {
            // A leave, drag, style change or close while creation was pending wins.
            if app
                .get_webview_window(super::usage_island::WINDOW_LABEL)
                .is_none()
            {
                let _ = handle.destroy();
                return;
            }
            if REVISION.load(Ordering::SeqCst) != revision {
                return;
            }
            let _ = handle.emit(EVENT, &snapshot);
        })
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tooltip_flips_and_clamps_at_screen_edges_at_every_island_scale() {
        for scale in [0.7, 1.0, 1.5, 3.0] {
            let area = PhysicalRect {
                position: PhysicalPosition::new(-1920, -200),
                size: PhysicalSize::new(1920, 1080),
            };
            for (x, y, expected) in [
                (-1900.0, -180.0, "below"),
                (-1000.0, 650.0, "above"),
                (-30.0, -180.0, "below"),
            ] {
                let anchor = PhysicalRect {
                    position: PhysicalPosition::new(x, y),
                    size: PhysicalSize::new(32.0 * scale, 32.0 * scale),
                };
                let size = PhysicalSize::new(276.0 * scale, 206.0 * scale);
                let (point, placement, arrow) = position(anchor, size, area, scale);
                assert_eq!(placement, expected);
                assert!(point.x >= area.position.x && point.y >= area.position.y);
                assert!(f64::from(point.x) + size.width + 24.0 * scale <= 0.0);
                assert!(f64::from(point.y) + size.height + 24.0 * scale <= 880.0);
                assert!((18.0..=258.0).contains(&arrow));
            }
        }
    }
    #[test]
    fn unknown_and_nonfinite_tooltip_geometry_is_rejected() {
        let mut request = Request {
            item: serde_json::json!({}),
            theme: "dark".into(),
            reset_label: String::new(),
            anchor: Anchor {
                x: 0.0,
                y: 0.0,
                width: 32.0,
                height: 32.0,
            },
            width: 276.0,
            height: 206.0,
            zoom: 1.0,
            side: false,
            clearance: None,
            direction: None,
        };
        assert!(valid(&request));
        request.zoom = f64::NAN;
        assert!(!valid(&request));
        request.zoom = 1.0;
        request.anchor.width = 0.0;
        assert!(!valid(&request));
    }
}
