use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, Paint, PaintStyle, Point};

use rustmotion_core::css::{CssStyle, FrameClock};
use rustmotion_core::engine::animator::{ease, AnimatedProperties};
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    build_shape_path, color4f_from_hex, draw_shape_path, draw_text_with_fallback, emoji_typeface,
    interpolate_path_data, measure_text_with_fallback, paint_from_hex, trim_path_between,
    typeface_with_fallback, wrap_text_with_tracking,
};
use rustmotion_core::expr::{Computed, Expr, Scope};
use rustmotion_core::schema::{
    EasingType, Fill, FontWeight, GradientType, LineCap, LineJoin, ShapeText, ShapeType, Stroke,
    TextAlign, TimelineStep,
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
    /// Where the visible stroke starts, as a fraction `0..1` of the path's own
    /// length — the counterpart to `draw_progress`, which only ever moves the
    /// end. The visible segment is the path between `draw_start` and
    /// `draw_progress`; a literal number or an `= expr` evaluated against
    /// `t`/`t_abs`/`duration`/`width`/`height`/`fps`, the same scope
    /// `dash_offset` reads. Has no effect unless `draw_progress` is also
    /// animating (or has already finished, in which case the segment simply
    /// erases from its tail) — matching `hold` on `iris`, this is a no-op by
    /// itself. Applied only to `stroke`; ignored for `fill`.
    #[serde(default)]
    pub draw_start: Option<Computed<f32>>,
    /// Keyframes on the shape's own path data (`d`), interpolated point by
    /// point when every keyframe shares the same command structure. A
    /// mismatched pair is reported on stderr and holds the earlier
    /// keyframe's shape rather than snapping — see
    /// `rustmotion_core::engine::renderer::interpolate_path_data`. Overrides
    /// `shape` entirely (for both `fill` and `stroke`) while present.
    #[serde(default)]
    pub path_morph: Option<PathMorph>,
}

/// A path's `d` animated over time by re-drawing the interpolated outline
/// between two authored keyframes, rather than a single frozen shape.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct PathMorph {
    /// At least one keyframe; a single keyframe holds that shape statically.
    pub keyframes: Vec<PathMorphKeyframe>,
    /// Applied within each segment between two consecutive keyframes.
    #[serde(default)]
    pub easing: EasingType,
    /// Loops back to the first keyframe once `time` passes the last one,
    /// instead of holding the last shape forever.
    #[serde(default)]
    pub repeat: bool,
    /// Only with `repeat`: alternates direction each cycle instead of
    /// snapping back to the first keyframe.
    #[serde(default)]
    pub yoyo: bool,
}

/// One stop in a `path_morph`.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct PathMorphKeyframe {
    /// Seconds from scene start, like every other keyframe `time` in this
    /// schema — not a `0..1` fraction.
    pub time: f64,
    /// SVG path data, same grammar as `shape: { "type": "path", "data": … }`.
    pub value: String,
}

rustmotion_core::impl_traits!(Shape {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

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

fn resolve_draw_start(value: &Option<Computed<f32>>, scope: &dyn Scope) -> f32 {
    let raw = match value {
        None => 0.0,
        Some(Computed::Literal(v)) => *v,
        Some(Computed::Expr(src)) => Expr::parse(src)
            .and_then(|e| e.eval(scope))
            .map(|v| v as f32)
            .unwrap_or(0.0),
    };
    raw.clamp(0.0, 1.0)
}

fn resolve_path_morph(morph: &PathMorph, time: f64) -> Option<skia_safe::Path> {
    let keyframes = &morph.keyframes;
    let first = keyframes.first()?;
    if keyframes.len() == 1 {
        return skia_safe::Path::from_svg(&first.value);
    }
    let last = keyframes.last()?;
    let first_time = first.time;
    let last_time = last.time;
    let span = (last_time - first_time).max(1e-9);

    let sample_time = if morph.repeat && time > first_time {
        let elapsed = time - first_time;
        let cycle = elapsed.rem_euclid(span);
        let forward = elapsed.div_euclid(span) as i64 % 2 == 0;
        if morph.yoyo && !forward {
            last_time - cycle
        } else {
            first_time + cycle
        }
    } else {
        time.clamp(first_time, last_time)
    };

    let mut lower = first;
    let mut upper = last;
    for pair in keyframes.windows(2) {
        if sample_time >= pair[0].time && sample_time <= pair[1].time {
            lower = &pair[0];
            upper = &pair[1];
            break;
        }
    }

    if lower.value == upper.value {
        return skia_safe::Path::from_svg(&lower.value);
    }
    let segment_span = (upper.time - lower.time).max(1e-9);
    let local_t = ((sample_time - lower.time) / segment_span).clamp(0.0, 1.0);
    let eased = ease(local_t, &morph.easing) as f32;
    interpolate_path_data(&lower.value, &upper.value, eased)
}

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

        let clock = frame_clock(ctx);
        let mut resolved_path = None;
        if let ShapeType::Path { data } = &self.shape {
            match resolve_template(data, &clock) {
                Ok(Some(resolved)) => resolved_path = Some(ShapeType::Path { data: resolved }),
                Ok(None) => {}
                Err(()) => return,
            }
        }
        let shape = resolved_path.as_ref().unwrap_or(&self.shape);
        let morphed_path = self
            .path_morph
            .as_ref()
            .and_then(|morph| resolve_path_morph(morph, clock.t));

        if let Some(fill) = &self.fill {
            let paint: Option<Paint> = match fill {
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
                match &morphed_path {
                    Some(path) => {
                        canvas.draw_path(path, &paint);
                    }
                    None => draw_shape_path(canvas, shape, 0.0, 0.0, w, h, corner_radius, &paint),
                };
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

            let expr_draw_start = resolve_draw_start(&self.draw_start, &clock);
            let draw_start = if props.draw_start >= 0.0 {
                props.draw_start.clamp(0.0, 1.0)
            } else {
                expr_draw_start
            };
            let draw_offset = props.draw_offset;
            let mut trimmed_path = None;

            if let Some(intervals) = stroke.dashed.as_ref().filter(|v| v.len() >= 2) {
                let phase = resolve_dash_offset(&stroke.dash_offset, &clock);
                if let Some(dash) = skia_safe::PathEffect::dash(intervals, phase) {
                    paint.set_path_effect(dash);
                }
            } else {
                let drawing = props.draw_progress >= 0.0 && props.draw_progress < 1.0;
                let trimming = draw_start > 0.0 || draw_offset.abs() > 0.0005;
                if drawing || trimming {
                    let end = if props.draw_progress >= 0.0 {
                        props.draw_progress.clamp(0.0, 1.0)
                    } else {
                        1.0
                    };
                    let trim_start = (draw_start + draw_offset).clamp(0.0, 1.0);
                    let trim_end = (end + draw_offset).clamp(0.0, 1.0);
                    if trim_end <= trim_start {
                        trimmed_path = Some(skia_safe::PathBuilder::new().detach());
                    } else {
                        let base = morphed_path
                            .clone()
                            .or_else(|| build_shape_path(shape, 0.0, 0.0, w, h, corner_radius));
                        if let Some(path) = base {
                            trimmed_path = Some(trim_path_between(&path, trim_start, trim_end));
                        }
                    }
                }
            }

            match trimmed_path.as_ref().or(morphed_path.as_ref()) {
                Some(path) => {
                    canvas.draw_path(path, &paint);
                }
                None => draw_shape_path(canvas, shape, 0.0, 0.0, w, h, corner_radius, &paint),
            };
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

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::traits::TimingConfig;
    use skia_safe::{surfaces, AlphaType, ColorType, ImageInfo};

    const W: i32 = 200;
    const H: i32 = 20;

    fn ctx_at(time: f64) -> PaintCtx {
        PaintCtx {
            time,
            scenario_time: time,
            scene_duration: 1.0,
            frame_index: 0,
            fps: 30,
            video_width: W as u32,
            video_height: H as u32,
            stagger_offset: 0.0,
        }
    }

    fn layout() -> BoxLayout {
        BoxLayout {
            width: W as f32,
            height: H as f32,
            ..Default::default()
        }
    }

    fn straight_line(color: &str) -> Shape {
        Shape {
            shape: ShapeType::Path {
                data: format!("M0 {} L{} {}", H / 2, W, H / 2),
            },
            text: None,
            timing: TimingConfig::default(),
            style: CssStyle::default(),
            timeline: vec![],
            stagger: None,
            fill: None,
            stroke: Some(Stroke {
                color: color.to_string(),
                width: 4.0,
                dashed: None,
                dash_offset: None,
                line_cap: LineCap::Butt,
                line_join: LineJoin::Miter,
            }),
            draw_start: None,
            path_morph: None,
        }
    }

    fn render_at(shape: &Shape, props: &AnimatedProperties, time: f64) -> Vec<u8> {
        let info = ImageInfo::new((W, H), ColorType::RGBA8888, AlphaType::Unpremul, None);
        let mut surface = surfaces::raster(&info, None, None).expect("raster surface");
        surface.canvas().clear(skia_safe::Color::BLACK);
        shape.paint_content(surface.canvas(), &layout(), props, &ctx_at(time));
        let row_bytes = W as usize * 4;
        let mut pixels = vec![0u8; row_bytes * H as usize];
        surface.read_pixels(&info, &mut pixels, row_bytes, (0, 0));
        pixels
    }

    fn render(shape: &Shape, props: &AnimatedProperties) -> Vec<u8> {
        render_at(shape, props, 0.0)
    }

    fn pixel(buf: &[u8], x: i32, y: i32) -> [u8; 4] {
        let base = ((y * W + x) * 4) as usize;
        [buf[base], buf[base + 1], buf[base + 2], buf[base + 3]]
    }

    fn is_red(px: [u8; 4]) -> bool {
        px[0] > 200 && px[1] < 40 && px[2] < 40
    }

    #[test]
    fn draw_start_leaves_no_ink_before_it() {
        let mut shape = straight_line("#FF0000");
        shape.draw_start = Some(Computed::Literal(0.5));
        let props = AnimatedProperties {
            draw_progress: 1.0,
            ..AnimatedProperties::default()
        };
        let out = render(&shape, &props);

        for x in [5, 40, 90] {
            assert!(
                !is_red(pixel(&out, x, H / 2)),
                "x={x} is before draw_start=0.5 on a 200px line and must have no ink"
            );
        }
        for x in [110, 150, 195] {
            assert!(
                is_red(pixel(&out, x, H / 2)),
                "x={x} is after draw_start=0.5 and before draw_progress=1.0, so it must be lit"
            );
        }
    }

    #[test]
    fn draw_start_absent_matches_the_pre_existing_draw_progress_behaviour() {
        let with_zero = {
            let mut shape = straight_line("#FF0000");
            shape.draw_start = Some(Computed::Literal(0.0));
            let props = AnimatedProperties {
                draw_progress: 0.5,
                ..AnimatedProperties::default()
            };
            render(&shape, &props)
        };
        let without_field = {
            let shape = straight_line("#FF0000");
            let props = AnimatedProperties {
                draw_progress: 0.5,
                ..AnimatedProperties::default()
            };
            render(&shape, &props)
        };
        assert_eq!(
            with_zero, without_field,
            "an explicit draw_start of 0.0 must render identically to draw_start absent"
        );
    }

    #[test]
    fn draw_start_without_an_active_draw_progress_erases_from_the_tail() {
        let mut shape = straight_line("#FF0000");
        shape.draw_start = Some(Computed::Literal(0.5));
        let props = AnimatedProperties::default();
        let out = render(&shape, &props);

        assert!(
            !is_red(pixel(&out, 20, H / 2)),
            "with draw_progress never animated, draw_start still trims the head of the stroke"
        );
        assert!(
            is_red(pixel(&out, 180, H / 2)),
            "the tail (up to the implicit end of 1.0) must remain lit"
        );
    }

    #[test]
    fn a_keyframe_driven_draw_start_is_honored_even_with_no_static_draw_start_field() {
        let shape = straight_line("#FF0000");
        let props = AnimatedProperties {
            draw_start: 0.5,
            draw_progress: 1.0,
            ..AnimatedProperties::default()
        };
        let out = render(&shape, &props);

        for x in [5, 40, 90] {
            assert!(
                !is_red(pixel(&out, x, H / 2)),
                "x={x} is before a keyframe-driven draw_start=0.5 and must have no ink — \
                 keyframes on draw_start must not be a no-op on shape"
            );
        }
        for x in [110, 150, 195] {
            assert!(
                is_red(pixel(&out, x, H / 2)),
                "x={x} is after the keyframe-driven draw_start=0.5 and must be lit"
            );
        }
    }

    #[test]
    fn draw_offset_marches_the_drawn_window_along_the_path() {
        let shape = straight_line("#FF0000");
        let no_offset = render(
            &shape,
            &AnimatedProperties {
                draw_progress: 0.3,
                ..AnimatedProperties::default()
            },
        );
        let offset = render(
            &shape,
            &AnimatedProperties {
                draw_progress: 0.3,
                draw_offset: 0.5,
                ..AnimatedProperties::default()
            },
        );

        assert!(
            is_red(pixel(&no_offset, 20, H / 2)),
            "with no offset the window starts at the path's own beginning"
        );
        assert!(
            !is_red(pixel(&offset, 20, H / 2)),
            "draw_offset=0.5 must march the window forward, leaving the path's start empty"
        );
        assert!(
            is_red(pixel(&offset, 140, H / 2)),
            "and paint further along the path instead"
        );
    }

    #[test]
    fn path_morph_interpolates_between_same_structure_paths() {
        let mut shape = straight_line("#FF0000");
        shape.stroke = None;
        shape.fill = Some(Fill::Solid("#FF0000".to_string()));
        shape.path_morph = Some(PathMorph {
            keyframes: vec![
                PathMorphKeyframe {
                    time: 0.0,
                    value: "M0 0 L10 0 L10 10 L0 10 Z".to_string(),
                },
                PathMorphKeyframe {
                    time: 1.0,
                    value: format!("M0 0 L{W} 0 L{W} {H} L0 {H} Z"),
                },
            ],
            easing: EasingType::Linear,
            repeat: false,
            yoyo: false,
        });

        let out = render_at(&shape, &AnimatedProperties::default(), 0.5);

        assert!(
            is_red(pixel(&out, 5, H / 2)),
            "the near corner must already be inside the halfway-grown rectangle"
        );
        assert!(
            !is_red(pixel(&out, W - 5, H / 2)),
            "the far corner must not be covered yet at t=0.5 of the morph"
        );
    }

    #[test]
    fn path_morph_with_a_structure_mismatch_does_not_panic_and_holds_a_shape() {
        let mut shape = straight_line("#FF0000");
        shape.stroke = None;
        shape.fill = Some(Fill::Solid("#FF0000".to_string()));
        shape.path_morph = Some(PathMorph {
            keyframes: vec![
                PathMorphKeyframe {
                    time: 0.0,
                    value: "M0 0 L10 0 L10 10 Z".to_string(),
                },
                PathMorphKeyframe {
                    time: 1.0,
                    value: "M0 0 L10 0 L10 10 L5 15 L0 10 Z".to_string(),
                },
            ],
            easing: EasingType::Linear,
            repeat: false,
            yoyo: false,
        });
        let _ = render_at(&shape, &AnimatedProperties::default(), 0.5);
    }
}
