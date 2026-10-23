//! The part of the window the interface may use. Android can lay the window out under the system
//! bars (edge to edge), and the activity reports the usable area as its content rect (in window
//! pixels; it shrinks for the soft keyboard too). The host gives egui that area as its screen rect,
//! so nothing is drawn or expected under a bar.

/// `content` (left, top, right, bottom, window pixels) as egui's screen rect in points, when it is
/// a proper part of the `window` rect; `None` when it is empty, not yet reported, or the whole window.
pub fn content_area(window: egui::Rect, content: (i32, i32, i32, i32), pixels_per_point: f32) -> Option<egui::Rect> {
    let (l, t, r, b) = content;
    if r <= l || b <= t || pixels_per_point.is_nan() || pixels_per_point <= 0.0 {
        return None;
    }
    let rect = egui::Rect::from_min_max(
        egui::pos2(l as f32 / pixels_per_point, t as f32 / pixels_per_point),
        egui::pos2(r as f32 / pixels_per_point, b as f32 / pixels_per_point),
    );
    // (within half a point of the window: the same thing)
    let inside =
        rect.min.x >= window.min.x - 0.5 && rect.min.y >= window.min.y - 0.5 && rect.max.x <= window.max.x + 0.5 && rect.max.y <= window.max.y + 0.5;
    let whole = (rect.min - window.min).length() < 0.5 && (rect.max - window.max).length() < 0.5;
    (inside && !whole && rect.width() >= 100.0 && rect.height() >= 100.0).then_some(rect)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn window() -> egui::Rect {
        egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0))
    }

    #[test]
    fn a_status_bar_takes_the_top_off() {
        let r = content_area(window(), (0, 72, 2560, 1600), 2.0).unwrap();
        assert_eq!(r, egui::Rect::from_min_max(egui::pos2(0.0, 36.0), egui::pos2(1280.0, 800.0)));
    }

    #[test]
    fn the_soft_keyboard_takes_the_bottom_off() {
        let r = content_area(window(), (0, 0, 2560, 900), 2.0).unwrap();
        assert_eq!(r.max.y, 450.0);
    }

    #[test]
    fn nothing_reported_or_the_whole_window_changes_nothing() {
        assert_eq!(content_area(window(), (0, 0, 0, 0), 2.0), None, "not reported yet");
        assert_eq!(content_area(window(), (0, 0, 2560, 1600), 2.0), None, "the whole window");
        assert_eq!(content_area(window(), (0, 0, 2600, 1600), 2.0), None, "bigger than the window: not this window's");
        assert_eq!(content_area(window(), (0, 0, 2560, 100), 2.0), None, "too small to lay anything out");
        assert_eq!(content_area(window(), (0, 72, 2560, 1600), 0.0), None, "no scale yet");
    }
}
