use skia_safe::{Canvas, Rect};

use rustmotion_core::css::style::{FontWeight as CssFontWeight, FontWeightKw};
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::paint_from_hex;
use rustmotion_core::schema::FontWeight;
use rustmotion_core::traits::PaintCtx;

use super::chrome::draw_chrome;
use super::diff::{determine_active_state, draw_diff_backgrounds, render_diff_transition};
use super::dimensions::{compute_code_dimensions, lerp};
use super::highlight::{get_theme, highlight_code, resolve_monospace_font};
use super::reveal::{compute_reveal, draw_highlighted_lines, draw_highlights, draw_line_numbers};
use super::Codeblock;

pub(super) fn render_codeblock(
    canvas: &Canvas,
    layer: &Codeblock,
    layout: &BoxLayout,
    _props: &AnimatedProperties,
    ctx: &PaintCtx,
) {
    let time = ctx.time;
    let font_family = layer.style.font_family_or("JetBrains Mono");
    // Resolved once, against the real per-frame viewport (`rem`/`vw`/`vh` on
    // `font-size` now resolve instead of silently dropping to 0px — lot B,
    // wave S) and threaded through every `compute_code_dimensions` call
    // below instead of each one re-deriving its own (previously identical
    // only by coincidence) value.
    let font_size = layer.style.font_size_px_ctx(
        &crate::intrinsic::font_size_ctx(ctx.video_width as f32, ctx.video_height as f32, 0.0),
        14.0,
    );
    let font_weight = match &layer.style.font_weight {
        Some(CssFontWeight::Keyword(FontWeightKw::Bold | FontWeightKw::Bolder)) => FontWeight::Bold,
        Some(CssFontWeight::Number(n)) if *n >= 600 => FontWeight::Bold,
        Some(CssFontWeight::Number(n)) => FontWeight::Weight(*n),
        _ => FontWeight::Normal,
    };
    let actual_line_height = layer.style.line_height_for(font_size);
    let Some(font) = resolve_monospace_font(font_family, font_size, font_weight) else {
        return;
    };
    let padding = {
        let (t, r, b, l) = layer.style.padding_px();
        if t == 0.0 && r == 0.0 && b == 0.0 && l == 0.0 {
            (16.0, 16.0, 16.0, 16.0)
        } else {
            (t, r, b, l)
        }
    };
    let theme = get_theme(&layer.theme);

    let (current_code, transition) = determine_active_state(layer, time);

    let chrome_enabled = layer.chrome.as_ref().is_some_and(|c| c.enabled);
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
        0.0
    };

    // The taffy layout sets the outer box size. The natural content height is
    // still computed (so we know whether to auto-scroll), but the box footprint
    // is taken from the laid-out BoxLayout, not the legacy `layer.size`.
    let natural_height = if let Some(ref trans) = transition {
        let dims_a = compute_code_dimensions(
            &trans.code_a,
            &font,
            font_size,
            padding,
            chrome_height,
            layer,
        );
        let dims_b = compute_code_dimensions(
            &trans.code_b,
            &font,
            font_size,
            padding,
            chrome_height,
            layer,
        );
        lerp(
            dims_a.total_height,
            dims_b.total_height,
            trans.progress as f32,
        )
    } else {
        compute_code_dimensions(
            &current_code,
            &font,
            font_size,
            padding,
            chrome_height,
            layer,
        )
        .total_height
    };

    let gutter_width = if max_gutter_width > 0.0 {
        max_gutter_width
    } else if let Some(ref trans) = transition {
        let dims_a = compute_code_dimensions(
            &trans.code_a,
            &font,
            font_size,
            padding,
            chrome_height,
            layer,
        );
        let dims_b = compute_code_dimensions(
            &trans.code_b,
            &font,
            font_size,
            padding,
            chrome_height,
            layer,
        );
        f32::max(dims_a.gutter_width, dims_b.gutter_width)
    } else {
        compute_code_dimensions(
            &current_code,
            &font,
            font_size,
            padding,
            chrome_height,
            layer,
        )
        .gutter_width
    };

    let total_width = layout.width.round();
    let total_height = layout.height.round();
    let x = layout.x;
    let y = layout.y;

    let (pad_top, pad_right, pad_bottom, pad_left) = padding;
    let corner_radius = layer.style.border_radius_px_or(12.0);

    let bg_color = layer.style.background_color_str().unwrap_or("#2b303b");
    let bg_paint = paint_from_hex(bg_color);
    let bg_rect = Rect::from_xywh(x, y, total_width, total_height);
    let rrect = skia_safe::RRect::new_rect_xy(bg_rect, corner_radius, corner_radius);

    canvas.save();
    canvas.clip_rrect(rrect, skia_safe::ClipOp::Intersect, true);

    canvas.draw_rect(bg_rect, &bg_paint);

    if chrome_enabled {
        draw_chrome(canvas, layer, x, y, total_width, corner_radius);
    }

    let code_x = x + pad_left + gutter_width;
    let code_y = y + chrome_height + pad_top;

    // #4: the non-transition (typewriter/reveal) path only paints
    // `visible_lines` lines, not the full `current_code` — `natural_height`
    // (used below) is the height of *all* the code, revealed or not. Using
    // it for the scroll offset made the offset constant and maximal from
    // t=0, translating the not-yet-revealed lines' eventual position
    // upward by the full amount immediately: the first lines to reveal sit
    // above the clip, invisible, until the reveal has caught up with that
    // fixed offset (reproduced: 60% of a 4s typewriter reveal painted zero
    // text pixels). Compute reveal state up front so the scroll offset can
    // be based on what's actually drawn — matches `terminal.rs`'s
    // `content_h = visible_lines * line_h + padding + chrome_h` formula,
    // which has never had this bug.
    let reveal_state = if transition.is_none() {
        let highlighted = highlight_code(&current_code, &layer.language, theme);
        let (visible_lines, visible_chars, last_line_opacity) =
            compute_reveal(layer, time, &highlighted);
        Some((highlighted, visible_lines, visible_chars, last_line_opacity))
    } else {
        None
    };

    // Diff transitions (`render_diff_transition`) always paint the entire
    // lerped diff, with no partial reveal — `natural_height` (the lerped
    // dims_a/dims_b height) already matches what gets drawn for that path,
    // so it needs no `visible_lines` adjustment; only the reveal path did.
    let drawn_height = match &reveal_state {
        Some((_, visible_lines, _, _)) => {
            *visible_lines as f32 * actual_line_height + pad_top + pad_bottom + chrome_height
        }
        None => natural_height,
    };

    let scroll_offset = if layer.auto_scroll {
        (drawn_height - total_height).max(0.0)
    } else {
        0.0
    };
    canvas.save();
    canvas.clip_rect(
        Rect::from_xywh(
            x,
            y + chrome_height,
            total_width,
            total_height - chrome_height,
        ),
        skia_safe::ClipOp::Intersect,
        true,
    );
    if scroll_offset > 0.0 {
        canvas.translate((0.0, -scroll_offset));
    }

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
        );
    } else {
        let (highlighted, visible_lines, visible_chars, last_line_opacity) =
            reveal_state.expect("reveal_state is always Some when transition is None");

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
    canvas.restore();
}
