use tauri::AppHandle;

use crate::{error::FlickError, models::SelectionRect};

#[derive(Debug, Clone, Copy)]
pub(super) struct CoordinateSpace {
    pub min_y: f64,
    pub max_y: f64,
}

#[derive(Debug, Clone)]
pub(super) struct OverlaySetup {
    pub geometry: Vec<SelectionRect>,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct OverlayVisuals {
    pub dim_alpha: f64,
    pub border_thickness: u32,
}

#[derive(Debug, Clone, Default)]
pub(super) struct OverlayDrawState {
    pub selection: Option<SelectionRect>,
    pub cursor: Option<(f64, f64)>,
}

pub(super) fn collect_overlay_setup(app: &AppHandle) -> Result<OverlaySetup, FlickError> {
    let geometry = app
        .available_monitors()?
        .into_iter()
        .map(|monitor| {
            let scale = monitor.scale_factor();
            let x = monitor.position().x as f64 / scale;
            let y = monitor.position().y as f64 / scale;
            let width = monitor.size().width as f64 / scale;
            let height = monitor.size().height as f64 / scale;
            SelectionRect {
                x: x.floor() as i32,
                y: y.floor() as i32,
                width: width.ceil() as u32,
                height: height.ceil() as u32,
            }
        })
        .collect::<Vec<_>>();
    Ok(OverlaySetup { geometry })
}

pub(super) fn border_rects(selection: SelectionRect, border_thickness: u32) -> [SelectionRect; 4] {
    let width = selection.width.max(border_thickness);
    let height = selection.height.max(border_thickness);

    [
        SelectionRect {
            x: selection.x,
            y: selection.y,
            width,
            height: border_thickness,
        },
        SelectionRect {
            x: selection.x,
            y: selection.y + height as i32 - border_thickness as i32,
            width,
            height: border_thickness,
        },
        SelectionRect {
            x: selection.x,
            y: selection.y,
            width: border_thickness,
            height,
        },
        SelectionRect {
            x: selection.x + width as i32 - border_thickness as i32,
            y: selection.y,
            width: border_thickness,
            height,
        },
    ]
}
