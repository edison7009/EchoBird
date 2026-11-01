use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalRect, PhysicalSize, WebviewWindow};

pub const WINDOW_LABEL: &str = "usage-island";
const COLLAPSED_SIZE: f64 = 60.0;
// The voice island keeps the bottom of the screen, so this one starts at the top.
const TOP_GAP: f64 = 24.0;

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
    centered: bool,
    area: &PhysicalRect<i32, u32>,
) -> PhysicalPosition<i32> {
    let (x, y) = if centered {
        (
            position.x + (current.width as i32 - target.width as i32) / 2,
            position.y + (current.height as i32 - target.height as i32) / 2,
        )
    } else {
        (position.x, position.y)
    };
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
    centered: bool,
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
        let next = fitted_position(position, current, target, centered, monitor.work_area());
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
                false,
                &area(),
            ),
            PhysicalPosition::new(-1000, 100)
        );
    }

    #[test]
    fn expansion_never_leaves_the_work_area() {
        assert_eq!(
            fitted_position(
                PhysicalPosition::new(-100, 1000),
                PhysicalSize::new(60, 60),
                PhysicalSize::new(400, 60),
                false,
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
            true,
            &area(),
        );
        assert_eq!(opened, PhysicalPosition::new(-1078, 222));
        assert_eq!(
            fitted_position(
                opened,
                PhysicalSize::new(216, 216),
                PhysicalSize::new(60, 60),
                true,
                &area(),
            ),
            PhysicalPosition::new(-1000, 300)
        );
    }
}
