use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Paint, PaintStyle, Point};

use rustmotion_core::css::{CssStyle, FrameClock};
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    build_shape_path, color4f_from_hex, draw_shape_path, draw_text_with_fallback, emoji_typeface,
    measure_text_with_fallback, paint_from_hex, typeface_with_fallback, wrap_text_with_tracking,
};
use rustmotion_core::expr::{Computed, Expr, Scope};
use rustmotion_core::schema::{
    Fill, FontWeight, GradientType, LineCap, LineJoin, ShapeText, ShapeType, Stroke, TextAlign,
    TimelineStep,
};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Shape {
    pub shape: ShapeType,
    #[serde(default)]
    pub text: Option<ShapeText>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
    #[serde(default)]
    pub fill: Option<Fill>,
    #[serde(default)]
    pub stroke: Option<Stroke>,
}

rustmotion_core::impl_traits!(Shape {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

/// This node's frame clock, reused as the `Scope` for both `stroke.
/// dash_offset` and any `${...}` marker in `path.data` — the same reserved
/// names (`$t`/`$T`/`$duration`/`$W`/`$H`/`$fps`) `CssStyle`'s own `opacity`/
/// `width`/`height` expressions already resolve against, via
/// `rustmotion_core::css::computed`'s doc. Neither `$i`/`$count`/`vars`/
/// `node(...)` is answered here: those are resolved as plain text before
/// this component ever deserializes (`for-each`'s own binding pass, at
/// load), so a marker or `dash_offset` naming one of them either arrives
/// already substituted or was never valid in the first place.
fn frame_clock(ctx: &PaintCtx) -> FrameClock {
    FrameClock {
        t: ctx.time,
        t_abs: ctx.scenario_time,
        duration: ctx.scene_duration,
        width: ctx.video_width as f64,
        height: ctx.video_height as f64,
        fps: ctx.fps as f64,
    }
}

fn skia_line_cap(cap: LineCap) -> skia_safe::PaintCap {
    match cap {
        LineCap::Butt => skia_safe::PaintCap::Butt,
        LineCap::Round => skia_safe::PaintCap::Round,
        LineCap::Square => skia_safe::PaintCap::Square,
    }
}

fn skia_line_join(join: LineJoin) -> skia_safe::PaintJoin {
    match join {
        LineJoin::Miter => skia_safe::PaintJoin::Miter,
        LineJoin::Round => skia_safe::PaintJoin::Round,
        LineJoin::Bevel => skia_safe::PaintJoin::Bevel,
    }
}

/// Resolve `stroke.dash_offset` against this frame's `scope`. Absent
/// (`None`) and a failed expression both fall back to `0.0` — the same
/// phase every dashed stroke had before this field existed — rather than
/// aborting the whole shape over one bad phase value; see `render_shape_
/// text` below for the same swallow-and-degrade convention already used in
/// this file, applied to a different `Result`.
fn resolve_dash_offset(offset: &Option<Computed<f32>>, scope: &dyn Scope) -> f32 {
    match offset {
        None => 0.0,
        Some(Computed::Literal(v)) => *v,
        Some(Computed::Expr(src)) => Expr::parse(src)
            .and_then(|e| e.eval(scope))
            .map(|v| v as f32)
            .unwrap_or(0.0),
    }
}

/// Resolve every `${...}` marker in a string into a literal, each marker's
/// contents evaluated as an expression (`rustmotion_core::expr`) against
/// `scope`. Used for both `path.data` (`"M ${...} ${...} L ..."`) and a
/// `fill`/`stroke` hex color built from computed channels
/// (`"#${r|02x}${g|02x}${b|02x}"`) — same mechanism, two string shapes.
///
/// Why markers, not the whole string as one `Computed<String>`: both
/// `path.data` and a hex color are prose with numbers embedded in them, not
/// a single value, and an expression only ever produces one `f64` — folding
/// the *entire* string as `"= ..."` could at best replace it with one
/// number, never a multi-point path or a `#rrggbb` triplet. `${...}` scopes
/// the expression grammar down to exactly the substrings that are numbers,
/// leaving the surrounding syntax (path-command letters, the `#` and digit
/// grouping) untouched.
///
/// A marker is `${expr}` (decimal, 4 places — what `path.data`'s
/// coordinates want) or `${expr|02x}` (the value rounded, clamped to
/// `0..=255`, and formatted as a zero-padded lowercase hex byte — what a
/// color channel wants). The `|` splits the two unambiguously: the
/// expression grammar's ternary is the only place it uses `:`, and it never
/// uses `|` at all. Any other format spec is a named failure, not a guess.
///
/// A `for-each`'s own `$i`/`$count`/item-field substitution
/// (`crate::variables`/`crate::expand`) already ran, textually, over the
/// whole document before this component ever deserialized — so by the time
/// a marker reaches here it holds only arithmetic plus whatever
/// `$t`/`$T`/`$W`/`$H`/`$fps`/`$duration` it still names, which `scope`
/// (this node's `FrameClock`) answers.
///
/// Returns `Ok(None)` when the string has no marker at all — the common,
/// zero-cost case; the caller keeps using it unchanged. Returns
/// `Ok(Some(resolved))` on success. Returns `Err(())` when a marker is
/// unterminated, its expression fails to parse or evaluate, or its format
/// spec is unrecognized: unlike `resolve_dash_offset`, this is not
/// swallowed to a placeholder, because splicing a wrong value into the
/// *middle* of path or color syntax can silently produce a
/// plausible-looking but wrong result (a stray point, an off-hue facet)
/// rather than an obviously-broken one — the caller skips painting instead.
fn resolve_template(s: &str, scope: &dyn Scope) -> std::result::Result<Option<String>, ()> {
    if !s.contains("${") {
        return Ok(None);
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    loop {
        match rest.find("${") {
            None => {
                out.push_str(rest);
                break;
            }
            Some(start) => {
                out.push_str(&rest[..start]);
                let after = &rest[start + 2..];
                let end = after.find('}').ok_or(())?;
                let marker = &after[..end];
                let (src, format_spec) = match marker.rsplit_once('|') {
                    Some((src, spec)) => (src, Some(spec)),
                    None => (marker, None),
                };
                let value = Expr::parse(src)
                    .map_err(|_| ())?
                    .eval(scope)
                    .map_err(|_| ())?;
                match format_spec {
                    None => out.push_str(&format!("{value:.4}")),
                    Some("02x") => {
                        let byte = value.round().clamp(0.0, 255.0) as i64;
                        out.push_str(&format!("{byte:02x}"));
                    }
                    Some(_) => return Err(()),
                }
                rest = &after[end + 1..];
            }
        }
    }
    Ok(Some(out))
}

impl Painter for Shape {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let w = layout.width;
        let h = layout.height;
        let corner_radius = self.style.border_radius_px();

        // `resolved_path` only ever holds something when `self.shape` is a
        // `ShapeType::Path` whose `data` carries a `${...}` marker — see
        // `resolve_template`'s own doc. Every other shape (and a `Path`
        // with no marker) leaves it `None` and `shape` below is a plain
        // reference to `self.shape`, unchanged from before this existed.
        let clock = frame_clock(ctx);
        let mut resolved_path = None;
        if let ShapeType::Path { data } = &self.shape {
            match resolve_template(data, &clock) {
                Ok(Some(resolved)) => resolved_path = Some(ShapeType::Path { data: resolved }),
                Ok(None) => {}
                // A marker didn't parse or evaluate: nothing valid to paint
                // this frame rather than a shape built from a half-spliced
                // string — see `resolve_template`'s doc for why this is not
                // swallowed to a placeholder the way `dash_offset` is.
                Err(()) => return,
            }
        }
        let shape = resolved_path.as_ref().unwrap_or(&self.shape);

        if let Some(fill) = &self.fill {
            let paint: Option<Paint> = match fill {
                // A `${...}`-marked solid color (e.g. a per-facet
                // `"#${r|02x}${g|02x}${b|02x}"` computed from an angle) that
                // fails to resolve just skips painting the fill — the
                // stroke/text below are independent layers and still paint.
                Fill::Solid(color) => match resolve_template(color, &clock) {
                    Ok(resolved) => Some(paint_from_hex(resolved.as_deref().unwrap_or(color))),
                    Err(()) => None,
                },
                Fill::Gradient(gradient) => Some({
                    let colors: Vec<skia_safe::Color4f> = gradient
                        .colors
                        .iter()
                        .map(|c| color4f_from_hex(c))
                        .collect();
                    // skia asserts `pos.len() == colors.len()` inside the gradient
                    // shader — a mismatch aborts the process instead of erroring.
                    // Nothing upstream enforces the pairing, so drop stops we
                    // cannot honour and let skia distribute the colours evenly.
                    let stops: Option<Vec<f32>> = gradient
                        .stops
                        .as_ref()
                        .filter(|s| s.len() == colors.len())
                        .cloned();
                    let mut paint = Paint::default();
                    paint.set_anti_alias(true);

                    let shader = match gradient.gradient_type {
                        GradientType::Linear => {
                            let angle = gradient.angle.unwrap_or(0.0);
                            let rad = angle.to_radians();
                            let cx = w / 2.0;
                            let cy = h / 2.0;
                            let dx = (w / 2.0) * rad.cos();
                            let dy = (h / 2.0) * rad.sin();
                            let start = Point::new(cx - dx, cy - dy);
                            let end = Point::new(cx + dx, cy + dy);
                            let gradient_colors = skia_safe::gradient::Colors::new(
                                &colors,
                                stops.as_deref(),
                                skia_safe::TileMode::Clamp,
                                Some(skia_safe::ColorSpace::new_srgb()),
                            );
                            let g = skia_safe::gradient::Gradient::new(
                                gradient_colors,
                                skia_safe::gradient::Interpolation::default(),
                            );
                            skia_safe::gradient::shaders::linear_gradient((start, end), &g, None)
                        }
                        GradientType::Radial => {
                            let center = Point::new(w / 2.0, h / 2.0);
                            let radius = w.max(h) / 2.0;
                            let gradient_colors = skia_safe::gradient::Colors::new(
                                &colors,
                                stops.as_deref(),
                                skia_safe::TileMode::Clamp,
                                Some(skia_safe::ColorSpace::new_srgb()),
                            );
                            let g = skia_safe::gradient::Gradient::new(
                                gradient_colors,
                                skia_safe::gradient::Interpolation::default(),
                            );
                            skia_safe::gradient::shaders::radial_gradient(
                                (center, radius),
                                &g,
                                None,
                            )
                        }
                    };
                    if let Some(shader) = shader {
                        paint.set_shader(shader);
                        paint.set_dither(true);
                    }
                    paint
                }),
            };
            if let Some(mut paint) = paint {
                paint.set_style(PaintStyle::Fill);
                draw_shape_path(canvas, shape, 0.0, 0.0, w, h, corner_radius, &paint);
            }
        }

        if let Some(stroke) = &self.stroke {
            let mut paint = paint_from_hex(&stroke.color);
            paint.set_style(PaintStyle::Stroke);
            let stroke_w = if props.stroke_width >= 0.0 {
                props.stroke_width
            } else {
                stroke.width
            };
            paint.set_stroke_width(stroke_w);
            paint.set_stroke_cap(skia_line_cap(stroke.line_cap));
            paint.set_stroke_join(skia_line_join(stroke.line_join));

            // An explicit `dashed` pattern is a deliberate authoring choice
            // — honour it (plus its `dash_offset` phase) over the
            // `draw_progress` reveal below, which would otherwise overwrite
            // the same `Paint::path_effect` slot with its own synthetic
            // two-interval dash and silently discard the author's pattern.
            if let Some(intervals) = stroke.dashed.as_ref().filter(|v| v.len() >= 2) {
                let phase = resolve_dash_offset(&stroke.dash_offset, &clock);
                if let Some(dash) = skia_safe::PathEffect::dash(intervals, phase) {
                    paint.set_path_effect(dash);
                }
            } else if props.draw_progress >= 0.0 && props.draw_progress < 1.0 {
                if let Some(path) = build_shape_path(shape, 0.0, 0.0, w, h, corner_radius) {
                    let mut measure = skia_safe::PathMeasure::new(&path, false, None);
                    let path_len = measure.length();
                    if path_len > 0.0 {
                        let draw_len = path_len * props.draw_progress.clamp(0.0, 1.0);
                        let intervals = [draw_len, path_len - draw_len + 0.01];
                        if let Some(dash) = skia_safe::PathEffect::dash(&intervals, 0.0) {
                            paint.set_path_effect(dash);
                        }
                    }
                }
            }

            draw_shape_path(canvas, shape, 0.0, 0.0, w, h, corner_radius, &paint);
        }

        if let Some(text) = &self.text {
            let _ = render_shape_text(canvas, text, 0.0, 0.0, w, h);
        }
    }
}

fn render_shape_text(
    canvas: &Canvas,
    text: &ShapeText,
    shape_x: f32,
    shape_y: f32,
    shape_w: f32,
    shape_h: f32,
) -> Result<()> {
    use rustmotion_core::schema::VerticalAlign;

    let pad = text.padding.unwrap_or(0.0);
    let area_x = shape_x + pad;
    let area_y = shape_y + pad;
    let area_w = shape_w - 2.0 * pad;
    let area_h = shape_h - 2.0 * pad;

    let font_style = match text.font_weight {
        FontWeight::Bold => skia_safe::FontStyle::bold(),
        FontWeight::Normal => skia_safe::FontStyle::normal(),
        FontWeight::Weight(w) => skia_safe::FontStyle::new(
            skia_safe::font_style::Weight::from(w as i32),
            skia_safe::font_style::Width::NORMAL,
            skia_safe::font_style::Slant::Upright,
        ),
    };

    let typeface = typeface_with_fallback(&text.font_family, font_style)?;

    let font = skia_safe::Font::from_typeface(typeface, text.font_size);
    let emoji_font = emoji_typeface().map(|tf| skia_safe::Font::from_typeface(tf, text.font_size));
    let (_strike_width, metrics) = font.metrics();
    let ascent = -metrics.ascent;
    let line_height = match text.line_height {
        Some(v) if v <= 10.0 => text.font_size * v,
        Some(v) => v,
        None => text.font_size * 1.3,
    };
    let letter_spacing = text.letter_spacing.unwrap_or(0.0);

    // Tracking-aware wrap (issue #125 §1): the fit test measures with the
    // same `letter_spacing` used below for `line_width`/`draw_text_with_
    // fallback`, so the wrap decision agrees with what's actually painted.
    // `ShapeText`'s `font_size`/`letter_spacing`/`line_height` are plain
    // `f32` (not `CssStyle`/`Length`), so issue #125 §2's relative-unit gap
    // doesn't apply here — there is no unit string to resolve.
    let lines = wrap_text_with_tracking(
        &text.content,
        &font,
        &emoji_font,
        Some(area_w),
        letter_spacing,
    );
    let descent = metrics.descent;
    let total_h = if lines.len() > 1 {
        (lines.len() - 1) as f32 * line_height + ascent + descent
    } else {
        ascent + descent
    };

    let y_start = match text.vertical_align {
        VerticalAlign::Top => area_y + ascent,
        VerticalAlign::Middle => area_y + (area_h - total_h) / 2.0 + ascent,
        VerticalAlign::Bottom => area_y + area_h - total_h + ascent,
    };

    let mut paint = paint_from_hex(&text.color);
    paint.set_alpha_f(1.0);

    for (i, line) in lines.iter().enumerate() {
        if line.is_empty() {
            continue;
        }

        let line_width = measure_text_with_fallback(line, &font, &emoji_font, letter_spacing);

        let x = match text.align {
            TextAlign::Left => area_x,
            TextAlign::Center => area_x + (area_w - line_width) / 2.0,
            TextAlign::Right => area_x + area_w - line_width,
        };
        let y = y_start + i as f32 * line_height;
        draw_text_with_fallback(
            canvas,
            line,
            &font,
            &emoji_font,
            letter_spacing,
            x,
            y,
            &paint,
        );
    }

    Ok(())
}
