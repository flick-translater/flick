use tauri::AppHandle;

use crate::{error::FlickError, models::SelectionRect};

#[derive(Debug, Clone)]
pub(super) struct OverlaySetup {
    pub geometry: Vec<SelectionRect>,
    pub desktop_bounds: SelectionRect,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct OverlayVisuals {
    pub dim_alpha: f64,
    pub border_thickness: u32,
}

pub(super) fn collect_overlay_setup(app: &AppHandle) -> Result<OverlaySetup, FlickError> {
    let geometry = app
        .available_monitors()?
        .into_iter()
        .map(|monitor| SelectionRect {
            x: monitor.position().x,
            y: monitor.position().y,
            width: monitor.size().width,
            height: monitor.size().height,
        })
        .collect::<Vec<_>>();

    let desktop_bounds = geometry
        .iter()
        .cloned()
        .reduce(union_rect)
        .ok_or_else(|| FlickError::Message("no monitors available for capture".into()))?;

    Ok(OverlaySetup {
        geometry,
        desktop_bounds,
    })
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

fn union_rect(a: SelectionRect, b: SelectionRect) -> SelectionRect {
    let left = a.x.min(b.x);
    let top = a.y.min(b.y);
    let right = (a.x + a.width as i32).max(b.x + b.width as i32);
    let bottom = (a.y + a.height as i32).max(b.y + b.height as i32);

    SelectionRect {
        x: left,
        y: top,
        width: (right - left) as u32,
        height: (bottom - top) as u32,
    }
}
