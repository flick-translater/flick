use anyhow::{Context, anyhow};
use image::{ImageBuffer, Rgba};
use x11rb::{
    connection::Connection,
    protocol::{
        randr::ConnectionExt as RandrExt,
        xproto::{ConnectionExt, ImageFormat, Setup, Visualid},
    },
};

use crate::models::SelectionRect;

use super::CachedScreenCapture;

pub fn capture_selection(
    selection: &SelectionRect,
    _cached_screens: &[CachedScreenCapture],
) -> anyhow::Result<ImageBuffer<Rgba<u8>, Vec<u8>>> {
    ensure_x11_session()?;

    let (conn, screen_num) = x11rb::connect(None).context("failed to connect to X11 display")?;
    let screen = &conn.setup().roots[screen_num];
    let desktop_origin = desktop_origin(&conn, screen.root).unwrap_or((0, 0));
    let x = selection
        .x
        .checked_sub(desktop_origin.0)
        .ok_or_else(|| anyhow!("selection starts outside root window"))?;
    let y = selection
        .y
        .checked_sub(desktop_origin.1)
        .ok_or_else(|| anyhow!("selection starts outside root window"))?;

    let reply = conn
        .get_image(
            ImageFormat::Z_PIXMAP,
            screen.root,
            x as i16,
            y as i16,
            selection.width as u16,
            selection.height as u16,
            u32::MAX,
        )
        .context("failed to issue X11 image request")?
        .reply()
        .context("failed to read X11 image reply")?;

    let bits_per_pixel = conn
        .setup()
        .pixmap_formats
        .iter()
        .find(|format| format.depth == reply.depth)
        .map(|format| usize::from(format.bits_per_pixel))
        .ok_or_else(|| anyhow!("missing X11 pixmap format for depth {}", reply.depth))?;
    let bytes_per_pixel = bits_per_pixel.div_ceil(8);

    let visual = find_visual(conn.setup(), screen.root_visual)
        .ok_or_else(|| anyhow!("failed to locate X11 root visual"))?;

    let mut rgba = vec![0u8; selection.width as usize * selection.height as usize * 4];
    let stride = selection.width as usize * bytes_per_pixel;

    for row in 0..selection.height as usize {
        let row_start = row * stride;
        let row_end = row_start + stride;
        let row_bytes = reply
            .data
            .get(row_start..row_end)
            .ok_or_else(|| anyhow!("unexpected X11 image buffer layout"))?;

        for column in 0..selection.width as usize {
            let pixel_start = column * bytes_per_pixel;
            let pixel_end = pixel_start + bytes_per_pixel;
            let pixel_bytes = &row_bytes[pixel_start..pixel_end];
            let pixel = decode_native_pixel(pixel_bytes);
            let [red, green, blue] =
                extract_rgb(pixel, visual.red_mask, visual.green_mask, visual.blue_mask);
            let out = (row * selection.width as usize + column) * 4;
            rgba[out] = red;
            rgba[out + 1] = green;
            rgba[out + 2] = blue;
            rgba[out + 3] = 255;
        }
    }

    ImageBuffer::<Rgba<u8>, _>::from_vec(selection.width, selection.height, rgba)
        .ok_or_else(|| anyhow!("failed to construct RGBA image buffer"))
}

fn ensure_x11_session() -> anyhow::Result<()> {
    if std::env::var_os("DISPLAY").is_none() {
        if std::env::var_os("WAYLAND_DISPLAY").is_some() {
            anyhow::bail!("Linux screenshot capture currently requires an X11 session");
        }
        anyhow::bail!("DISPLAY is not available for Linux screenshot capture");
    }
    Ok(())
}

fn desktop_origin<C: Connection>(conn: &C, root: u32) -> anyhow::Result<(i32, i32)> {
    let monitors = conn
        .randr_get_monitors(root, true)
        .context("failed to query RandR monitors")?
        .reply()
        .context("failed to read RandR monitor reply")?;

    let min_x = monitors
        .monitors
        .iter()
        .map(|monitor| i32::from(monitor.x))
        .min()
        .unwrap_or(0);
    let min_y = monitors
        .monitors
        .iter()
        .map(|monitor| i32::from(monitor.y))
        .min()
        .unwrap_or(0);

    Ok((min_x, min_y))
}

fn find_visual(setup: &Setup, visual_id: Visualid) -> Option<&x11rb::protocol::xproto::Visualtype> {
    setup.roots.iter().find_map(|screen| {
        screen.allowed_depths.iter().find_map(|depth| {
            depth
                .visuals
                .iter()
                .find(|visual| visual.visual_id == visual_id)
        })
    })
}

fn decode_native_pixel(bytes: &[u8]) -> u32 {
    bytes.iter().enumerate().fold(0u32, |value, (index, byte)| {
        value | (u32::from(*byte) << (index * 8))
    })
}

fn extract_rgb(pixel: u32, red_mask: u32, green_mask: u32, blue_mask: u32) -> [u8; 3] {
    [
        extract_component(pixel, red_mask),
        extract_component(pixel, green_mask),
        extract_component(pixel, blue_mask),
    ]
}

fn extract_component(pixel: u32, mask: u32) -> u8 {
    if mask == 0 {
        return 0;
    }

    let shift = mask.trailing_zeros();
    let raw = (pixel & mask) >> shift;
    let max = mask >> shift;
    if max == 0 {
        return 0;
    }

    ((raw * 255) / max) as u8
}
