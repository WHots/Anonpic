//! Geometry helpers for translating input into capture regions.

use windows_sys::Win32::Foundation::{LPARAM, POINT, RECT};

/// Extracts the signed client coordinates packed into a mouse `lParam`.
pub(super) fn lparam_to_point(lparam: LPARAM) -> POINT
{
    let x = (lparam & 0xFFFF) as i16 as i32;
    let y = ((lparam >> 16) & 0xFFFF) as i16 as i32;
    POINT { x, y }
}


/// Builds a rectangle whose edges are ordered from two arbitrary points.
pub(super) fn normalized_rect(a: POINT, b: POINT) -> RECT
{
    RECT
    {
        left: a.x.min(b.x),
        top: a.y.min(b.y),
        right: a.x.max(b.x),
        bottom: a.y.max(b.y),
    }
}


/// Builds a circle's bounding square and clamps its radius within `bounds`.
pub(super) fn circle_rect(center: POINT, edge: POINT, bounds: &RECT) -> RECT
{
    let dx = (edge.x - center.x) as f64;
    let dy = (edge.y - center.y) as f64;
    let distance = (dx * dx + dy * dy).sqrt();

    let max_radius = (center.x - bounds.left).min(bounds.right - center.x).min(center.y - bounds.top).min(bounds.bottom - center.y).max(0);
    let radius = distance.min(max_radius as f64) as i32;

    RECT
    {
        left: center.x - radius,
        top: center.y - radius,
        right: center.x + radius,
        bottom: center.y + radius,
    }
}

#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn decodes_signed_mouse_coordinates()
    {
        let packed = ((34_i16 as u16 as isize) << 16) | (-12_i16 as u16 as isize);
        let point = lparam_to_point(packed);

        assert_eq!((point.x, point.y), (-12, 34));
    }


    #[test]
    fn normalizes_reversed_points()
    {
        let region = normalized_rect(POINT { x: 40, y: 30 }, POINT { x: 10, y: 5 });

        assert_eq!((region.left, region.top, region.right, region.bottom), (10, 5, 40, 30));
    }


    #[test]
    fn clamps_circle_to_bounds()
    {
        let bounds = RECT { left: 0, top: 0, right: 100, bottom: 80 };
        let region = circle_rect(POINT { x: 20, y: 40 }, POINT { x: 90, y: 40 }, &bounds);

        assert_eq!((region.left, region.top, region.right, region.bottom), (0, 20, 40, 60));
    }
}
