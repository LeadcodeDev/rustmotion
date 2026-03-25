mod chrome;
mod diff;
mod dimensions;
mod highlight;
mod reveal;

use crate::error::Result;
use skia_safe::{Canvas, Rect};

use super::renderer::paint_from_hex;
use crate::engine::animator::AnimatedProperties;
use crate::components::Codeblock;
use crate::schema::{FontWeight, Spacing, VideoConfig};

use chrome::draw_chrome;
use diff::{determine_active_state, draw_diff_backgrounds, render_diff_transition};
use dimensions::{compute_code_dimensions, lerp};
use highlight::{get_theme, highlight_code, resolve_monospace_font};
use reveal::{compute_reveal, draw_highlights, draw_highlighted_lines, draw_line_numbers};

// ─── Main render entry point ─────────────────────────────────────────────────

pub fn render_codeblock(
    canvas: &Canvas,
    layer: &Codeblock,
    _config: &VideoConfig,
    time: f64,
    _props: &AnimatedProperties,
) -> Result<()> {
    let font_family = layer
        .style
        .font_family
        .as_deref()
        .unwrap_or("JetBrains Mono");
    let font_size = layer.style.font_size.unwrap_or(14.0);
    let font_weight = layer
        .style
        .font_weight
        .clone()
        .unwrap_or(FontWeight::Normal);
    let line_height = layer.style.line_height.unwrap_or(1.5);
    let font = resolve_monospace_font(font_family, font_size, font_weight);
    let actual_line_height = font_size * line_height;
    let padding = layer
        .style
        .padding
        .as_ref()
        .cloned()
        .unwrap_or(Spacing::Uniform(16.0));
    let theme = get_theme(&layer.theme);

    let (current_code, transition) = determine_active_state(layer, time);

    let chrome_enabled = layer.chrome.as_ref().map_or(false, |c| c.enabled);
    let chrome_height = if chrome_enabled { 36.0 } else { 0.0 };

    // Pre-compute the max gutter width across all states so line numbers
    // never cause a sudden horizontal shift when a transition starts.
    let max_gutter_width = if layer.show_line_numbers && !layer.states.is_empty() {
        let max_lines = std::iter::once(layer.code.lines().count())
            .chain(layer.states.iter().map(|s| s.code.lines().count()))
            .max()
            .unwrap_or(1)
            .max(1);
        let digits = format!("{}", max_lines).len();
        let digit_width = font.measure_str("0", None).0;
        (digits as f32 * digit_width) + 24.0
    } else {
        0.0 // will be computed per-code below
    };

    // Compute dimensions — interpolate during transitions
    let (total_width, total_height, gutter_width) = if let Some(ref trans) = transition {
        let dims_a = compute_code_dimensions(&trans.code_a, &font, &padding, chrome_height, layer);
        let dims_b = compute_code_dimensions(&trans.code_b, &font, &padding, chrome_height, layer);
        let p = trans.progress as f32;
        let gutter = if max_gutter_width > 0.0 {
            max_gutter_width
        } else {
            f32::max(dims_a.gutter_width, dims_b.gutter_width)
        };
        match &layer.size {
            Some(s) => (s.width, s.height, gutter),
            None => (
                lerp(dims_a.total_width, dims_b.total_width, p),
                lerp(dims_a.total_height, dims_b.total_height, p),
                gutter,
            ),
        }
    } else {
        let dims = compute_code_dimensions(&current_code, &font, &padding, chrome_height, layer);
        let gutter = if max_gutter_width > 0.0 {
            max_gutter_width
        } else {
            dims.gutter_width
        };
        match &layer.size {
            Some(s) => (s.width, s.height, gutter),
            None => (dims.total_width, dims.total_height, gutter),
        }
    };

    // Snap to integer pixel boundaries to eliminate sub-pixel anti-aliasing
    // artifacts on the edges (jagged/crenelated borders)
    let x = 0.0f32;
    let y = 0.0f32;
    let total_width = total_width.round();
    let total_height = total_height.round();

    let (pad_top, pad_right, _pad_bottom, pad_left) = padding.resolve();
    let corner_radius = layer.style.border_radius.unwrap_or(12.0);

    let bg_color = layer.style.background.as_deref().unwrap_or("#2b303b");
    let bg_paint = paint_from_hex(bg_color);
    let bg_rect = Rect::from_xywh(x, y, total_width, total_height);
    let rrect = skia_safe::RRect::new_rect_xy(bg_rect, corner_radius, corner_radius);

    canvas.save();
    canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);

    // Background (plain rect — the clip handles the rounding)
    canvas.draw_rect(bg_rect, &bg_paint);

    // Chrome (title bar)
    if chrome_enabled {
        draw_chrome(canvas, layer, x, y, total_width, corner_radius);
    }

    // Code area
    let code_x = x + pad_left + gutter_width;
    let code_y = y + chrome_height + pad_top;

    if let Some(ref trans) = transition {
        render_diff_transition(
            canvas,
            layer,
            &font,
            theme,
            code_x,
            code_y,
            actual_line_height,
            gutter_width,
            pad_left,
            x,
            trans,
        )?;
    } else {
        let highlighted = highlight_code(&current_code, &layer.language, theme);
        let (visible_lines, visible_chars, last_line_opacity) =
            compute_reveal(layer, time, &highlighted);

        if layer.show_line_numbers {
            draw_line_numbers(
                canvas,
                &font,
                x + pad_left,
                code_y,
                actual_line_height,
                visible_lines,
            );
        }

        draw_highlights(
            canvas,
            &layer.highlights,
            time,
            x + pad_left,
            code_y,
            actual_line_height,
            total_width - pad_left - pad_right,
        );

        // Diff mode: draw colored backgrounds for +/- lines
        if layer.diff {
            draw_diff_backgrounds(
                canvas,
                &current_code,
                x + pad_left,
                code_y,
                actual_line_height,
                total_width - pad_left - pad_right,
                visible_lines,
            );
        }

        draw_highlighted_lines(
            canvas,
            &highlighted,
            &font,
            code_x,
            code_y,
            actual_line_height,
            visible_lines,
            visible_chars,
            last_line_opacity,
        );
    }

    canvas.restore();
    Ok(())
}

/// V2 entry point: render a Codeblock component at the current canvas origin.
/// Render a v2 Codeblock component.
pub fn render_codeblock_v2(
    canvas: &Canvas,
    cb: &crate::components::Codeblock,
    time: f64,
) -> Result<()> {
    use crate::engine::animator::AnimatedProperties;

    let dummy_config = crate::schema::VideoConfig {
        width: 1920,
        height: 1080,
        fps: 30,
        background: "#000000".to_string(),
        codec: None,
        crf: None,
    };

    render_codeblock(
        canvas,
        cb,
        &dummy_config,
        time,
        &AnimatedProperties::default(),
    )
}
