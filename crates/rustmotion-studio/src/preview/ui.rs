use std::time::Instant;

use skia_safe::{Paint, Path, Rect};

// ── Constants ───────────────────────────────────────────────────────

pub(super) const CONTROLS_BAR_W_RATIO: f32 = 0.45;
pub(super) const CONTROLS_BAR_MIN_W: f32 = 280.0;
pub(super) const CONTROLS_BAR_MARGIN_BOTTOM: f32 = 24.0;
pub(super) const CONTROLS_BAR_RADIUS: f32 = 14.0;
pub(super) const CONTROLS_BAR_PAD_X: f32 = 20.0;
pub(super) const CONTROLS_BAR_PAD_Y: f32 = 12.0;
pub(super) const BUTTON_SIZE: f32 = 32.0;
pub(super) const BUTTON_ICON_SIZE: f32 = 14.0;
pub(super) const BUTTON_GAP: f32 = 16.0;
pub(super) const TIMELINE_BAR_H: f32 = 4.0;
pub(super) const TIMELINE_BAR_RADIUS: f32 = 2.0;
pub(super) const TIMELINE_ROW_H: f32 = 20.0;
pub(super) const EXPORT_BTN_W: f32 = 32.0;
pub(super) const EXPORT_BTN_H: f32 = 32.0;

// ── Layout ──────────────────────────────────────────────────────────

pub(super) struct ControlBarLayout {
    pub bar_rect: Rect,
    pub prev_btn: Rect,
    pub play_btn: Rect,
    pub next_btn: Rect,
    pub export_btn: Rect,
    pub timeline_rect: Rect,
    pub time_left_pos: (f32, f32),
    pub time_right_pos: (f32, f32),
}

pub(super) fn compute_control_bar_layout(width: f32, height: f32) -> ControlBarLayout {
    let bar_w = (width * CONTROLS_BAR_W_RATIO).max(CONTROLS_BAR_MIN_W).min(width - 40.0);
    let buttons_row_h = BUTTON_SIZE;
    let bar_h = CONTROLS_BAR_PAD_Y + buttons_row_h + 8.0 + TIMELINE_ROW_H + CONTROLS_BAR_PAD_Y;
    let bar_x = (width - bar_w) / 2.0;
    let bar_y = height - CONTROLS_BAR_MARGIN_BOTTOM - bar_h;
    let bar_rect = Rect::from_xywh(bar_x, bar_y, bar_w, bar_h);

    let btn_row_cy = bar_y + CONTROLS_BAR_PAD_Y + buttons_row_h / 2.0;
    let btn_y = btn_row_cy - BUTTON_SIZE / 2.0;
    let total_btns_w = 3.0 * BUTTON_SIZE + 2.0 * BUTTON_GAP;
    let btn_x0 = bar_x + (bar_w - total_btns_w) / 2.0;
    let prev_btn = Rect::from_xywh(btn_x0, btn_y, BUTTON_SIZE, BUTTON_SIZE);
    let play_btn = Rect::from_xywh(btn_x0 + BUTTON_SIZE + BUTTON_GAP, btn_y, BUTTON_SIZE, BUTTON_SIZE);
    let next_btn = Rect::from_xywh(btn_x0 + 2.0 * (BUTTON_SIZE + BUTTON_GAP), btn_y, BUTTON_SIZE, BUTTON_SIZE);
    let export_btn = Rect::from_xywh(
        bar_x + bar_w - CONTROLS_BAR_PAD_X - EXPORT_BTN_W,
        btn_row_cy - EXPORT_BTN_H / 2.0,
        EXPORT_BTN_W,
        EXPORT_BTN_H,
    );

    let row2_y = bar_y + CONTROLS_BAR_PAD_Y + buttons_row_h + 8.0;
    let row2_cy = row2_y + TIMELINE_ROW_H / 2.0;
    let time_label_w = 44.0;
    let tl_x = bar_x + CONTROLS_BAR_PAD_X + time_label_w + 8.0;
    let tl_right = bar_x + bar_w - CONTROLS_BAR_PAD_X - time_label_w - 8.0;
    let tl_w = (tl_right - tl_x).max(0.0);
    let timeline_rect = Rect::from_xywh(tl_x, row2_cy - TIMELINE_BAR_H / 2.0, tl_w, TIMELINE_BAR_H);

    let time_baseline_y = row2_cy + 4.0;
    let time_left_pos = (bar_x + CONTROLS_BAR_PAD_X, time_baseline_y);
    let time_right_pos = (bar_x + bar_w - CONTROLS_BAR_PAD_X, time_baseline_y);

    ControlBarLayout {
        bar_rect,
        prev_btn,
        play_btn,
        next_btn,
        export_btn,
        timeline_rect,
        time_left_pos,
        time_right_pos,
    }
}

pub(super) fn timeline_x_to_frame(x: f64, tl_left: f64, tl_right: f64, total: u32) -> u32 {
    let bar_w = tl_right - tl_left;
    if bar_w <= 0.0 {
        return 0;
    }
    let ratio = ((x - tl_left) / bar_w).clamp(0.0, 1.0);
    ((ratio * total as f64) as u32).min(total.saturating_sub(1))
}

pub(super) fn rect_contains(rect: &Rect, x: f32, y: f32) -> bool {
    x >= rect.left && x <= rect.right && y >= rect.top && y <= rect.bottom
}

// ── Drawing helpers ─────────────────────────────────────────────────

pub(super) fn draw_play_icon(canvas: &skia_safe::Canvas, rect: &Rect, paint: &Paint) {
    let cx = rect.center_x();
    let cy = rect.center_y();
    let s = BUTTON_ICON_SIZE / 2.0;
    let mut path = Path::new();
    path.move_to((cx - s * 0.5, cy - s));
    path.line_to((cx + s * 0.8, cy));
    path.line_to((cx - s * 0.5, cy + s));
    path.close();
    canvas.draw_path(&path, paint);
}

pub(super) fn draw_pause_icon(canvas: &skia_safe::Canvas, rect: &Rect, paint: &Paint) {
    let cx = rect.center_x();
    let cy = rect.center_y();
    let s = BUTTON_ICON_SIZE / 2.0;
    let bar_w = s * 0.45;
    let gap = s * 0.25;
    canvas.draw_rect(
        Rect::from_xywh(cx - gap - bar_w, cy - s, bar_w, s * 2.0),
        paint,
    );
    canvas.draw_rect(
        Rect::from_xywh(cx + gap, cy - s, bar_w, s * 2.0),
        paint,
    );
}

pub(super) fn draw_prev_icon(canvas: &skia_safe::Canvas, rect: &Rect, paint: &Paint) {
    let cx = rect.center_x();
    let cy = rect.center_y();
    let s = BUTTON_ICON_SIZE / 2.0;
    canvas.draw_rect(
        Rect::from_xywh(cx - s * 0.8, cy - s * 0.7, 2.0, s * 1.4),
        paint,
    );
    let mut path = Path::new();
    path.move_to((cx + s * 0.6, cy - s * 0.7));
    path.line_to((cx - s * 0.4, cy));
    path.line_to((cx + s * 0.6, cy + s * 0.7));
    path.close();
    canvas.draw_path(&path, paint);
}

pub(super) fn draw_next_icon(canvas: &skia_safe::Canvas, rect: &Rect, paint: &Paint) {
    let cx = rect.center_x();
    let cy = rect.center_y();
    let s = BUTTON_ICON_SIZE / 2.0;
    let mut path = Path::new();
    path.move_to((cx - s * 0.6, cy - s * 0.7));
    path.line_to((cx + s * 0.4, cy));
    path.line_to((cx - s * 0.6, cy + s * 0.7));
    path.close();
    canvas.draw_path(&path, paint);
    canvas.draw_rect(
        Rect::from_xywh(cx + s * 0.8 - 2.0, cy - s * 0.7, 2.0, s * 1.4),
        paint,
    );
}

pub(super) fn draw_export_icon(canvas: &skia_safe::Canvas, rect: &Rect, paint: &Paint) {
    let cx = rect.center_x();
    let cy = rect.center_y();
    let s = BUTTON_ICON_SIZE / 2.0;
    let mut path = Path::new();
    path.move_to((cx, cy - s * 0.8));
    path.line_to((cx, cy + s * 0.3));
    path.move_to((cx - s * 0.5, cy - s * 0.1));
    path.line_to((cx, cy + s * 0.5));
    path.line_to((cx + s * 0.5, cy - s * 0.1));
    let mut stroke_paint = paint.clone();
    stroke_paint.set_style(skia_safe::PaintStyle::Stroke);
    stroke_paint.set_stroke_width(1.8);
    stroke_paint.set_stroke_cap(skia_safe::paint::Cap::Round);
    stroke_paint.set_stroke_join(skia_safe::paint::Join::Round);
    canvas.draw_path(&path, &stroke_paint);
    let mut tray = Path::new();
    tray.move_to((cx - s * 0.7, cy + s * 0.7));
    tray.line_to((cx - s * 0.7, cy + s * 0.9));
    tray.line_to((cx + s * 0.7, cy + s * 0.9));
    tray.line_to((cx + s * 0.7, cy + s * 0.7));
    canvas.draw_path(&tray, &stroke_paint);
}

// ── Export state ─────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum ExportState {
    Idle,
    Exporting,
    Done(Instant),
    Error(Instant),
}
