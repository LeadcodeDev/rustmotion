use skia_safe::{Canvas, ColorType, ImageInfo, Paint};

use crate::schema::{
    AnimatedBackground, BackgroundPreset, ConcentricCirclesConfig, GradientShiftConfig,
    GradientType, GridDotsConfig, HaloConfig, HaloZone, HeropatternConfig, ScrollDirection,
};
use rustmotion_core::engine::renderer::{color4f_from_hex, paint_from_hex};

/// Draw an animated background (gradient, concentric circles, grid dots, halo, or heropattern).
pub(super) fn draw_animated_background(
    canvas: &Canvas,
    bg: &AnimatedBackground,
    time: f32,
    width: f32,
    height: f32,
) {
    // Compute scroll offset for tiled presets (gradient_shift handles rotation internally)
    let (scroll_x, scroll_y) = compute_scroll_offset(bg, time);

    // Whole tile periods the wrap removed. The geometry doesn't care (the
    // pattern is periodic on `spacing`) but `draw_bg_grid_dots`'s pulse is
    // a function of position, so without adding these back its phase would
    // jump by `spacing * 0.01` radians every time the offset wraps — a
    // visible, periodic pop in every dot's radius and alpha at once.
    let (raw_x, raw_y) = raw_scroll_offset(bg, time);
    let phase_origin = (raw_x - scroll_x, raw_y - scroll_y);

    canvas.save();
    canvas.translate((bg.x + scroll_x, bg.y + scroll_y));

    match &bg.preset {
        BackgroundPreset::GradientShift(cfg) => draw_bg_gradient_shift(
            canvas,
            cfg,
            bg.speed,
            bg.direction.as_ref(),
            time,
            width,
            height,
        ),
        BackgroundPreset::GridDots(cfg) => {
            draw_bg_grid_dots(canvas, cfg, time, width, height, phase_origin)
        }
        BackgroundPreset::ConcentricCircles(cfg) => {
            draw_bg_concentric_circles(canvas, cfg, bg.speed, time, width, height)
        }
        BackgroundPreset::Halo(cfg) => draw_bg_halo(canvas, cfg, bg.speed, time, width, height),
        BackgroundPreset::Heropattern(cfg) => draw_bg_heropattern(canvas, cfg, time, width, height),
    }

    canvas.restore();
}

/// Draw animated background with camera parallax for world views.
/// Offsets the grid pattern by (cam_x, cam_y) modulo spacing so that
/// the texture scrolls as the camera pans.
pub(super) fn draw_world_bg_with_parallax(
    canvas: &Canvas,
    bg: &AnimatedBackground,
    time: f32,
    width: f32,
    height: f32,
    cam_x: f32,
    cam_y: f32,
) {
    match &bg.preset {
        BackgroundPreset::Halo(cfg) => {
            let world_w = width * 5.0;
            let world_h = height * 5.0;
            canvas.save();
            canvas.translate((-cam_x, -cam_y));
            draw_bg_halo(canvas, cfg, bg.speed, time, world_w, world_h);
            canvas.restore();
        }
        _ => {
            // Grid-based backgrounds: modulo offset for seamless tiling.
            let spacing = tile_spacing(&bg.preset);
            let offset_x = -(cam_x % spacing);
            let offset_y = -(cam_y % spacing);
            canvas.save();
            canvas.translate((offset_x, offset_y));
            draw_animated_background(
                canvas,
                bg,
                time,
                width + spacing * 2.0,
                height + spacing * 2.0,
            );
            canvas.restore();
        }
    }
}

fn draw_bg_gradient_shift(
    canvas: &Canvas,
    cfg: &GradientShiftConfig,
    speed: f32,
    direction: Option<&ScrollDirection>,
    time: f32,
    width: f32,
    height: f32,
) {
    use skia_safe::{gradient_shader::GradientShaderColors, Point};

    if cfg.colors.len() < 2 {
        return;
    }

    let base_colors: Vec<skia_safe::Color4f> =
        cfg.colors.iter().map(|c| color4f_from_hex(c)).collect();

    // Direction determines rotation sense; default is cw
    let sign = match direction {
        Some(ScrollDirection::Ccw) => -1.0,
        _ => 1.0,
    };
    let angle = (sign * speed * time) % 360.0;
    let rad = angle.to_radians();

    // Subdivide color stops (16 intermediate steps between each pair),
    // interpolating in *linear light* and re-encoding to sRGB per generated
    // stop — see `subdivide_gradient_stops` for why this can't be delegated
    // to a Skia `ColorSpace` tag on the shader (the render surfaces carry no
    // color space, so any such tag is a silent no-op).
    let (colors, positions) = subdivide_gradient_stops(&base_colors, 16);

    let shader = match cfg.gradient_type {
        GradientType::Linear => {
            let cx = width / 2.0;
            let cy = height / 2.0;
            let half_diag = (width.powi(2) + height.powi(2)).sqrt() / 2.0;
            let start = Point::new(cx - rad.cos() * half_diag, cy - rad.sin() * half_diag);
            let end = Point::new(cx + rad.cos() * half_diag, cy + rad.sin() * half_diag);
            skia_safe::shader::Shader::linear_gradient(
                (start, end),
                GradientShaderColors::ColorsInSpace(&colors, None),
                Some(&positions[..]),
                skia_safe::TileMode::Clamp,
                None,
                None,
            )
        }
        GradientType::Radial => {
            let center = Point::new(width / 2.0, height / 2.0);
            let radius = width.max(height) / 2.0;
            skia_safe::shader::Shader::radial_gradient(
                center,
                radius,
                GradientShaderColors::ColorsInSpace(&colors, None),
                Some(&positions[..]),
                skia_safe::TileMode::Clamp,
                None,
                None,
            )
        }
    };

    if let Some(shader) = shader {
        let mut paint = Paint::default();
        paint.set_shader(shader);
        paint.set_dither(true);
        canvas.draw_rect(skia_safe::Rect::from_wh(width, height), &paint);
    }
}

/// Subdivide gradient color stops by inserting intermediate interpolated
/// colors. Returns (colors, positions) with `subdivisions` extra stops
/// between each original pair.
///
/// RGB is interpolated in **linear light** (decoded from sRGB, lerped,
/// re-encoded to sRGB per generated stop) — the actual fix for
/// rules/gradient-quality.md's "linear color space interpolation" claim.
/// The two mitigations used to rely on Skia: tagging the shader's colors
/// with `ColorSpace::new_srgb_linear()` and subdividing so Skia's own
/// per-pixel lerp had more (supposedly linear-space) stops to work with.
/// Both were silent no-ops: the render surfaces are created with no color
/// space (`ImageInfo::new(..., None)`), which short-circuits any
/// color-space conversion Skia would otherwise do — so the colors stayed
/// gamma-encoded sRGB the whole time, and subdividing an already-sRGB lerp
/// is a mathematical identity (17x more stops, zero visual effect). Doing
/// the gamma conversion here, on plain `f32`s, works regardless of what
/// color space (if any) the destination surface ends up tagged with later.
///
/// Alpha is NOT gamma-encoded and keeps a plain linear lerp.
pub(super) fn subdivide_gradient_stops(
    colors: &[skia_safe::Color4f],
    subdivisions: u32,
) -> (Vec<skia_safe::Color4f>, Vec<f32>) {
    let n = colors.len();
    if n < 2 {
        return (colors.to_vec(), vec![0.0]);
    }
    let total = (n - 1) * subdivisions as usize + n;
    let mut out_colors = Vec::with_capacity(total);
    let mut out_pos = Vec::with_capacity(total);
    let seg = (n - 1) as f32;

    for i in 0..n - 1 {
        let c0 = &colors[i];
        let c1 = &colors[i + 1];
        let steps = subdivisions + 1;
        for s in 0..steps {
            let t = s as f32 / steps as f32;
            let global_t = (i as f32 + t) / seg;
            let color = if t == 0.0 {
                // Exact copy at the segment start — no conversion round-trip
                // drift on stops that already existed pre-subdivision.
                *c0
            } else {
                skia_safe::Color4f {
                    r: lerp_srgb_channel(c0.r, c1.r, t),
                    g: lerp_srgb_channel(c0.g, c1.g, t),
                    b: lerp_srgb_channel(c0.b, c1.b, t),
                    a: c0.a + (c1.a - c0.a) * t,
                }
            };
            out_colors.push(color);
            out_pos.push(global_t);
        }
    }
    // Last color — exact copy, same reasoning as the `t == 0.0` case above.
    out_colors.push(colors[n - 1]);
    out_pos.push(1.0);

    (out_colors, out_pos)
}

/// Lerp one sRGB-encoded channel (0..1) by decoding both endpoints to linear
/// light, interpolating there, and re-encoding back to sRGB.
fn lerp_srgb_channel(a: f32, b: f32, t: f32) -> f32 {
    let linear = srgb_to_linear(a) + (srgb_to_linear(b) - srgb_to_linear(a)) * t;
    linear_to_srgb(linear)
}

/// sRGB EOTF (decode): gamma-encoded 0..1 -> linear light 0..1.
fn srgb_to_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// sRGB OETF (encode): linear light 0..1 -> gamma-encoded 0..1.
fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// Soft colored glow zones (halo preset).
fn draw_bg_halo(canvas: &Canvas, cfg: &HaloConfig, speed: f32, time: f32, width: f32, height: f32) {
    for (i, zone) in cfg.zones.iter().enumerate() {
        let cx = zone.x * width;
        let cy = zone.y * height;
        let base_radius = zone.radius * width.max(height);
        // Each zone gets a unique phase and slightly different frequency.
        let phase =
            (zone.x * 17.3 + zone.y * 31.7 + i as f32 * 0.73).fract() * std::f32::consts::TAU;
        // `speed` is shared with the scrolling presets, where it means pixels
        // per second and defaults to 30. Used directly as an angular frequency
        // that is 30 rad/s — a ~5 Hz strobe, not a glow. BREATH_RATE converts
        // it into a slow ambient pulse: at the default it gives a period of
        // roughly 10 seconds, which reads as light rather than as flicker.
        const BREATH_RATE: f32 = 0.02;
        let freq = speed * BREATH_RATE * (0.7 + (zone.x * 13.1 + zone.y * 7.9).fract() * 0.6);
        let breath = 1.0 + 0.15 * (time * freq + phase).sin();
        let radius = base_radius * breath;

        let mut color = color4f_from_hex(&zone.color);
        // `opacity` multiplies whatever alpha `color` already carries (opaque
        // by default, or hex-encoded, e.g. `#1E3A8A55`). Default 1.0 is a
        // true no-op — it leaves the colour's own alpha untouched.
        color.a *= zone.opacity.clamp(0.0, 1.0);
        let mut paint = Paint::default();
        paint.set_anti_alias(true);
        paint.set_color4f(color, None);
        paint.set_mask_filter(skia_safe::MaskFilter::blur(
            skia_safe::BlurStyle::Normal,
            radius * 0.15,
            false,
        ));
        canvas.draw_circle((cx, cy), radius, &paint);
    }
}

/// Expanding concentric circles from center.
fn draw_bg_concentric_circles(
    canvas: &Canvas,
    cfg: &ConcentricCirclesConfig,
    speed: f32,
    time: f32,
    width: f32,
    height: f32,
) {
    use skia_safe::PaintStyle;

    let mut paint = paint_from_hex(&cfg.color);
    paint.set_style(PaintStyle::Stroke);
    paint.set_stroke_width(cfg.element_size);
    paint.set_anti_alias(true);

    let cx = width / 2.0;
    let cy = height / 2.0;
    let max_radius = (width.powi(2) + height.powi(2)).sqrt() / 2.0;
    let spacing = if let Some(count) = cfg.count {
        if count > 0 {
            max_radius / count as f32
        } else {
            cfg.spacing.max(20.0)
        }
    } else {
        cfg.spacing.max(20.0)
    };
    let offset = (time * speed) % spacing;

    let mut r = offset;
    while r < max_radius {
        // Fade out as circles expand
        let alpha = 1.0 - (r / max_radius).clamp(0.0, 1.0);
        paint.set_alpha_f(alpha * 0.3);
        canvas.draw_circle((cx, cy), r, &paint);
        r += spacing;
    }
}

/// Pulse factor (radius multiplier and alpha basis) of the dot drawn at
/// canvas-local `(x, y)`.
///
/// `phase_origin` is the whole number of tile periods `compute_scroll_offset`
/// wrapped away, and is subtracted so the argument stays the dot's position
/// on the *unwrapped* scroll track. Geometry is periodic on `spacing` and so
/// survives the wrap unchanged; this `sin` is not, and would otherwise step
/// by `spacing * 0.01` rad at every wrap.
fn dot_pulse(x: f32, y: f32, time: f32, phase_origin: (f32, f32)) -> f32 {
    let wx = x - phase_origin.0;
    let wy = y - phase_origin.1;
    (wx * 0.01 + wy * 0.01 + time * 2.0).sin() * 0.3 + 0.7
}

/// Animated dot grid pattern.
fn draw_bg_grid_dots(
    canvas: &Canvas,
    cfg: &GridDotsConfig,
    time: f32,
    width: f32,
    height: f32,
    phase_origin: (f32, f32),
) {
    let mut paint = paint_from_hex(&cfg.color);
    paint.set_anti_alias(true);

    let spacing = cfg.spacing.max(20.0);
    let dot_radius = cfg.element_size / 2.0;

    // Scroll is handled by compute_scroll_offset + canvas translate
    // upstream, which now wraps the offset into `(-spacing, spacing)` (see
    // `compute_scroll_offset`) — this loop must overscan symmetrically on
    // BOTH axes (one `spacing` of margin on every side) to still cover the
    // full viewport for any offset in that range. The x-loop used to start
    // at 0 with no left margin (asymmetric vs. the y-loop below), so any
    // positive scroll left a growing blank band on the left edge.
    let mut y = -spacing;
    while y < height + spacing {
        let mut x = -spacing;
        while x < width + spacing {
            // Pulse: subtle size variation based on position + time
            let phase = dot_pulse(x, y, time, phase_origin);
            let r = dot_radius * phase;
            paint.set_alpha_f(phase * 0.4);
            canvas.draw_circle((x, y), r, &paint);
            x += spacing;
        }
        y += spacing;
    }
}

/// Tiled heropattern background.
fn draw_bg_heropattern(
    canvas: &Canvas,
    cfg: &HeropatternConfig,
    _time: f32,
    width: f32,
    height: f32,
) {
    let Some(def) = crate::engine::heropatterns::find_pattern(&cfg.pattern) else {
        return;
    };

    let tile_w = def.width * cfg.scale;
    let tile_h = def.height * cfg.scale;
    if tile_w < 1.0 || tile_h < 1.0 {
        return;
    }

    // Build the SVG source with color/opacity substituted
    let svg_content = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">{}</svg>"#,
        def.width,
        def.height,
        def.width,
        def.height,
        def.svg_paths
            .replace("{{color}}", &cfg.color)
            .replace("{{opacity}}", &cfg.opacity.to_string()),
    );

    // Render one tile via usvg/resvg
    let opt = usvg::Options::default();
    let Ok(tree) = usvg::Tree::from_data(svg_content.as_bytes(), &opt) else {
        return;
    };

    let pw = def.width.ceil() as u32;
    let ph = def.height.ceil() as u32;
    let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
        return;
    };
    resvg::render(&tree, tiny_skia::Transform::default(), &mut pixmap.as_mut());

    // Convert to Skia image
    let info = ImageInfo::new(
        (pw as i32, ph as i32),
        ColorType::RGBA8888,
        skia_safe::AlphaType::Premul,
        None,
    );
    let row_bytes = pw as usize * 4;
    let Some(tile_image) = skia_safe::images::raster_from_data(
        &info,
        skia_safe::Data::new_copy(pixmap.data()),
        row_bytes,
    ) else {
        return;
    };

    // Build a tiled shader from the tile image
    let matrix = if cfg.scale != 1.0 {
        Some(skia_safe::Matrix::scale((cfg.scale, cfg.scale)))
    } else {
        None
    };
    let Some(shader) = tile_image.to_shader(
        (skia_safe::TileMode::Repeat, skia_safe::TileMode::Repeat),
        skia_safe::SamplingOptions::default(),
        matrix.as_ref(),
    ) else {
        return;
    };

    let mut paint = Paint::default();
    paint.set_shader(shader);
    paint.set_anti_alias(true);

    let margin = tile_w.max(tile_h);
    canvas.draw_rect(
        skia_safe::Rect::from_xywh(
            -margin,
            -margin,
            width + margin * 2.0,
            height + margin * 2.0,
        ),
        &paint,
    );
}

/// Interpolate two AnimatedBackground structs. `t` goes from 0.0 (fully `a`) to 1.0 (fully `b`).
#[allow(dead_code)]
pub(super) fn interpolate_animated_bg(
    a: &AnimatedBackground,
    b: &AnimatedBackground,
    t: f32,
) -> AnimatedBackground {
    let lerp = |x: f32, y: f32| x * (1.0 - t) + y * t;

    fn lerp_colors(
        a_colors: &[String],
        b_colors: &[String],
        lerp: impl Fn(f32, f32) -> f32,
    ) -> Vec<String> {
        let max = a_colors.len().max(b_colors.len());
        let mut out = Vec::with_capacity(max);
        for i in 0..max {
            let ca = a_colors.get(i).map(|c| color4f_from_hex(c));
            let cb = b_colors.get(i).map(|c| color4f_from_hex(c));
            match (ca, cb) {
                (Some(ca), Some(cb)) => {
                    out.push(format!(
                        "#{:02X}{:02X}{:02X}{:02X}",
                        (lerp(ca.r, cb.r) * 255.0) as u8,
                        (lerp(ca.g, cb.g) * 255.0) as u8,
                        (lerp(ca.b, cb.b) * 255.0) as u8,
                        (lerp(ca.a, cb.a) * 255.0) as u8
                    ));
                }
                (None, Some(_)) => out.push(b_colors[i].clone()),
                (Some(_), None) => out.push(a_colors[i].clone()),
                (None, None) => {}
            }
        }
        out
    }

    fn lerp_zones(
        a_zones: &[HaloZone],
        b_zones: &[HaloZone],
        lerp: impl Fn(f32, f32) -> f32,
    ) -> Vec<HaloZone> {
        let max = a_zones.len().max(b_zones.len());
        let mut out = Vec::with_capacity(max);
        for i in 0..max {
            match (a_zones.get(i), b_zones.get(i)) {
                (Some(za), Some(zb)) => {
                    let ca = color4f_from_hex(&za.color);
                    let cb = color4f_from_hex(&zb.color);
                    out.push(HaloZone {
                        color: format!(
                            "#{:02X}{:02X}{:02X}",
                            (lerp(ca.r, cb.r) * 255.0) as u8,
                            (lerp(ca.g, cb.g) * 255.0) as u8,
                            (lerp(ca.b, cb.b) * 255.0) as u8
                        ),
                        x: lerp(za.x, zb.x),
                        y: lerp(za.y, zb.y),
                        radius: lerp(za.radius, zb.radius),
                        opacity: lerp(za.opacity, zb.opacity),
                    });
                }
                (None, Some(zb)) => out.push(zb.clone()),
                (Some(za), None) => out.push(za.clone()),
                (None, None) => {}
            }
        }
        out
    }

    // Interpolate preset — same type: interpolate fields, different type: snap at t >= 0.5
    let preset = match (&a.preset, &b.preset) {
        (BackgroundPreset::GradientShift(ac), BackgroundPreset::GradientShift(bc)) => {
            BackgroundPreset::GradientShift(GradientShiftConfig {
                colors: lerp_colors(&ac.colors, &bc.colors, lerp),
                gradient_type: bc.gradient_type.clone(),
            })
        }
        (BackgroundPreset::GridDots(ac), BackgroundPreset::GridDots(bc)) => {
            let ca = color4f_from_hex(&ac.color);
            let cb = color4f_from_hex(&bc.color);
            BackgroundPreset::GridDots(GridDotsConfig {
                color: format!(
                    "#{:02X}{:02X}{:02X}{:02X}",
                    (lerp(ca.r, cb.r) * 255.0) as u8,
                    (lerp(ca.g, cb.g) * 255.0) as u8,
                    (lerp(ca.b, cb.b) * 255.0) as u8,
                    (lerp(ca.a, cb.a) * 255.0) as u8
                ),
                element_size: lerp(ac.element_size, bc.element_size),
                spacing: lerp(ac.spacing, bc.spacing),
            })
        }
        (BackgroundPreset::ConcentricCircles(ac), BackgroundPreset::ConcentricCircles(bc)) => {
            let ca = color4f_from_hex(&ac.color);
            let cb = color4f_from_hex(&bc.color);
            BackgroundPreset::ConcentricCircles(ConcentricCirclesConfig {
                color: format!(
                    "#{:02X}{:02X}{:02X}{:02X}",
                    (lerp(ca.r, cb.r) * 255.0) as u8,
                    (lerp(ca.g, cb.g) * 255.0) as u8,
                    (lerp(ca.b, cb.b) * 255.0) as u8,
                    (lerp(ca.a, cb.a) * 255.0) as u8
                ),
                element_size: lerp(ac.element_size, bc.element_size),
                spacing: lerp(ac.spacing, bc.spacing),
                count: bc.count,
            })
        }
        (BackgroundPreset::Halo(ac), BackgroundPreset::Halo(bc)) => {
            BackgroundPreset::Halo(HaloConfig {
                zones: lerp_zones(&ac.zones, &bc.zones, lerp),
            })
        }
        _ => {
            // Different preset types: snap to b at midpoint
            if t >= 0.5 {
                b.preset.clone()
            } else {
                a.preset.clone()
            }
        }
    };

    AnimatedBackground {
        preset,
        x: lerp(a.x, b.x),
        y: lerp(a.y, b.y),
        speed: lerp(a.speed, b.speed),
        direction: b.direction.clone(),
    }
}

/// Tile period (px) a preset's own draw loop repeats on — the amount by
/// which a scroll offset can be wrapped without changing the rendered
/// pattern. Shared by `compute_scroll_offset` (below) and
/// `draw_world_bg_with_parallax`'s camera-pan modulo so the two never
/// diverge on what "one period" means for a given preset.
fn tile_spacing(preset: &BackgroundPreset) -> f32 {
    match preset {
        BackgroundPreset::GridDots(cfg) => cfg.spacing.max(20.0),
        BackgroundPreset::ConcentricCircles(cfg) => cfg.spacing.max(20.0),
        BackgroundPreset::Heropattern(cfg) => {
            let def = crate::engine::heropatterns::find_pattern(&cfg.pattern);
            def.map(|d| d.width * cfg.scale).unwrap_or(60.0).max(20.0)
        }
        _ => 60.0_f32.max(20.0),
    }
}

/// Compute the scroll offset for tiled backgrounds based on direction +
/// speed, wrapped into `(-spacing, spacing)` so it never grows unbounded.
///
/// Bug this fixes: the offset used to grow linearly with `time` forever.
/// The tiled draw loops (`draw_bg_grid_dots`, `draw_bg_heropattern`) only
/// ever overscan by one `spacing`/`margin` around the viewport — with the
/// canvas translated by an unbounded offset, the pattern slides off-frame
/// and leaves a growing blank band once the offset exceeds that one-tile
/// margin (see paint.md finding #5). Since every tiled pattern is exactly
/// periodic on `spacing`, translating by any offset congruent mod `spacing`
/// produces byte-identical pixels — Rust's `%` already returns a value with
/// `|result| < spacing` and the same sign as the input, which is exactly
/// the symmetric `(-spacing, spacing)` margin the (now-symmetric, see
/// `draw_bg_grid_dots`) draw loops need. `t=0` (or `speed=0`) stays an exact
/// `(0.0, 0.0)` no-op — `0.0 % spacing == 0.0`.
pub(super) fn compute_scroll_offset(bg: &AnimatedBackground, time: f32) -> (f32, f32) {
    let (raw_x, raw_y) = raw_scroll_offset(bg, time);
    let spacing = tile_spacing(&bg.preset);
    (raw_x % spacing, raw_y % spacing)
}

/// The unwrapped scroll offset — how far the pattern *would* have travelled
/// under the old unbounded scheme. Only `compute_scroll_offset` (for the
/// wrap) and the grid-dot pulse phase (for continuity across a wrap, see
/// `phase_origin` in `draw_animated_background`) need this; nothing should
/// translate a canvas by it.
fn raw_scroll_offset(bg: &AnimatedBackground, time: f32) -> (f32, f32) {
    let speed = bg.speed;
    if speed == 0.0 {
        return (0.0, 0.0);
    }
    let (dx, dy) = match bg.direction.as_ref() {
        Some(ScrollDirection::Up) => (0.0, -1.0),
        Some(ScrollDirection::Down) => (0.0, 1.0),
        Some(ScrollDirection::Left) => (-1.0, 0.0),
        Some(ScrollDirection::Right) => (1.0, 0.0),
        Some(ScrollDirection::UpLeft) => (-0.707, -0.707),
        Some(ScrollDirection::UpRight) => (0.707, -0.707),
        Some(ScrollDirection::DownLeft) => (-0.707, 0.707),
        Some(ScrollDirection::DownRight) => (0.707, 0.707),
        _ => (0.0, 0.0), // Cw/Ccw handled inside gradient_shift
    };
    (dx * speed * time, dy * speed * time)
}

#[cfg(test)]
mod halo_opacity_tests {
    use crate::encode::video::{build_frame_tasks, render_frame_task, FrameTask};
    use crate::loader::load_scenario_from_source;

    /// Render frame 0 of the first scene in a scenario JSON string.
    fn render_first_frame(json: &str) -> Vec<u8> {
        let scenario = load_scenario_from_source(None, Some(json)).expect("load");
        let tasks = build_frame_tasks(&scenario);
        let task = tasks
            .iter()
            .find(|t| matches!(t, FrameTask::Normal { .. }))
            .expect("normal task");
        render_frame_task(&scenario.video, &scenario, task).expect("render")
    }

    /// A single centered white halo zone on a black ground, `opacity` templated in.
    fn halo_scenario(opacity_field: &str) -> String {
        format!(
            r##"{{"video":{{"width":100,"height":100,"background":"#000000"}},
                "scenes":[{{"duration":1.0,
                    "background":{{"preset":"halo","speed":0,
                        "zones":[{{"color":"#FFFFFF","x":0.5,"y":0.5,"radius":0.3{opacity_field}}}]}}
                    ,"children":[]}}]}}"##
        )
    }

    fn center_rgba(buf: &[u8], width: u32) -> (u8, u8, u8, u8) {
        let base = (50 * width as usize + 50) * 4;
        (buf[base], buf[base + 1], buf[base + 2], buf[base + 3])
    }

    #[test]
    fn opacity_defaults_to_1_and_is_a_true_noop() {
        // Explicit 1.0 and an entirely absent field must render byte-identically:
        // multiplying an f32 alpha by 1.0 is an exact IEEE-754 identity, so this
        // also proves the default composes as a no-op with the color's own alpha.
        let with_field = render_first_frame(&halo_scenario(r#","opacity":1.0"#));
        let without_field = render_first_frame(&halo_scenario(""));
        assert_eq!(
            with_field, without_field,
            "default opacity must be pixel-identical to an explicit 1.0"
        );
    }

    #[test]
    fn opacity_scales_alpha_monotonically() {
        let buf_1_0 = render_first_frame(&halo_scenario(r#","opacity":1.0"#));
        let buf_0_5 = render_first_frame(&halo_scenario(r#","opacity":0.5"#));
        let buf_0_2 = render_first_frame(&halo_scenario(r#","opacity":0.2"#));

        let (r1, ..) = center_rgba(&buf_1_0, 100);
        let (r05, ..) = center_rgba(&buf_0_5, 100);
        let (r02, ..) = center_rgba(&buf_0_2, 100);

        assert!(
            r1 > r05 && r05 > r02,
            "expected monotonic falloff: opacity=1.0 -> {r1}, 0.5 -> {r05}, 0.2 -> {r02}"
        );
        // White zone over a black ground: center pixel ≈ 255 * opacity.
        assert_eq!(r1, 255);
        assert!(
            (r05 as i32 - 128).abs() <= 2,
            "0.5 opacity center was {r05}"
        );
        assert!((r02 as i32 - 51).abs() <= 2, "0.2 opacity center was {r02}");
    }

    #[test]
    fn opacity_multiplies_the_colors_own_hex_alpha() {
        // #1E3A8A55 already carries alpha 0x55 (~0.333). opacity 0.5 must
        // multiply through to an effective alpha of ~0.1667, not override it.
        let bg = "#05060A";
        let scenario = format!(
            r##"{{"video":{{"width":100,"height":100,"background":"{bg}"}},
                "scenes":[{{"duration":1.0,
                    "background":{{"preset":"halo","speed":0,
                        "zones":[{{"color":"#1E3A8A55","x":0.5,"y":0.5,"radius":0.35,"opacity":0.5}}]}}
                    ,"children":[]}}]}}"##
        );
        let buf = render_first_frame(&scenario);
        let (r, g, b, _a) = center_rgba(&buf, 100);

        let effective_alpha = (0x55 as f32 / 255.0) * 0.5;
        let expect = |fg: u8, bgc: u8| -> f32 {
            fg as f32 * effective_alpha + bgc as f32 * (1.0 - effective_alpha)
        };
        let (er, eg, eb) = (expect(0x1E, 0x05), expect(0x3A, 0x06), expect(0x8A, 0x0A));

        assert!((r as f32 - er).abs() <= 3.0, "r={r} expected~{er}");
        assert!((g as f32 - eg).abs() <= 3.0, "g={g} expected~{eg}");
        assert!((b as f32 - eb).abs() <= 3.0, "b={b} expected~{eb}");
    }

    #[test]
    fn opacity_is_clamped_to_0_1_range() {
        let over_one = render_first_frame(&halo_scenario(r#","opacity":2.5"#));
        let clamped_one = render_first_frame(&halo_scenario(r#","opacity":1.0"#));
        assert_eq!(
            over_one, clamped_one,
            "opacity > 1.0 must clamp to the same result as 1.0"
        );

        let negative = render_first_frame(&halo_scenario(r#","opacity":-1.0"#));
        let (r, g, b, _a) = center_rgba(&negative, 100);
        // Fully clamped to 0 alpha: only the black scene background shows through.
        assert_eq!(
            (r, g, b),
            (0, 0, 0),
            "negative opacity must clamp to fully transparent"
        );
    }
}

#[cfg(test)]
mod scroll_offset_wrap_tests {
    //! TDD tests for paint.md finding #5: `compute_scroll_offset` must wrap
    //! into one tile period instead of growing unbounded, or the tiled
    //! background's draw loops (which only ever overscan by one `spacing`
    //! around the viewport) leave a growing blank band.

    use super::*;
    use crate::schema::GridDotsConfig;

    fn grid_bg(direction: ScrollDirection, speed: f32) -> AnimatedBackground {
        AnimatedBackground {
            preset: BackgroundPreset::GridDots(GridDotsConfig {
                color: "#ffffff".into(),
                element_size: 8.0,
                spacing: 40.0,
            }),
            x: 0.0,
            y: 0.0,
            speed,
            direction: Some(direction),
        }
    }

    #[test]
    fn scroll_offset_stays_within_one_tile_period() {
        // Repro (paint.md #5): 300x200, grid_dots, speed 60, direction
        // right, spacing 40 — at t=3s the raw offset is 180px (4.5
        // spacings), way outside what `draw_bg_grid_dots`'s one-tile
        // overscan margin can cover.
        let bg = grid_bg(ScrollDirection::Right, 60.0);
        for t in [0.0f32, 0.1, 0.5, 1.0, 3.0, 10.0, 37.3] {
            let (dx, dy) = compute_scroll_offset(&bg, t);
            assert!(
                (-40.0..=40.0).contains(&dx),
                "t={t}: dx={dx} must stay within one tile period (±spacing=40)"
            );
            assert_eq!(
                dy, 0.0,
                "t={t}: pure horizontal scroll must not drift vertically (dy={dy})"
            );
        }
    }

    #[test]
    fn scroll_offset_at_t0_is_unchanged_zero() {
        // Non-regression: t=0 must stay an exact no-op, not jump to a whole
        // tile period ahead/behind.
        let bg = grid_bg(ScrollDirection::Right, 60.0);
        assert_eq!(compute_scroll_offset(&bg, 0.0), (0.0, 0.0));
    }

    #[test]
    fn scroll_offset_zero_speed_is_still_a_pure_noop() {
        let bg = grid_bg(ScrollDirection::Right, 0.0);
        assert_eq!(compute_scroll_offset(&bg, 5.0), (0.0, 0.0));
    }

    /// Regression guard for a side effect of the wrap itself: geometry is
    /// periodic on `spacing` so it crosses a wrap unchanged, but the dot
    /// pulse is a `sin` of position and is not. Feeding it canvas-local
    /// coordinates made every dot's radius and alpha step at once, once per
    /// `spacing / speed` seconds.
    #[test]
    fn dot_pulse_is_continuous_across_a_wrap() {
        let bg = grid_bg(ScrollDirection::Right, 60.0);
        // spacing 40 / speed 60 => the offset wraps at t = 2/3 s.
        let (before, after) = (0.6666_f32, 0.6667_f32);
        assert!(
            compute_scroll_offset(&bg, before).0 > compute_scroll_offset(&bg, after).0,
            "test setup: these two instants must straddle a wrap"
        );

        // Pulse of whichever dot lands on a fixed screen position.
        let (sx_screen, sy_screen) = (200.0_f32, 100.0_f32);
        let sampled = |t: f32| {
            let (sx, sy) = compute_scroll_offset(&bg, t);
            let (rx, ry) = raw_scroll_offset(&bg, t);
            dot_pulse(sx_screen - sx, sy_screen - sy, t, (rx - sx, ry - sy))
        };
        let delta = (sampled(after) - sampled(before)).abs();
        assert!(
            delta < 0.01,
            "pulse must not jump across a wrap, got delta {delta}"
        );

        // Witness that this is a real hazard and not a vacuous assertion:
        // the same sample without the phase origin does step visibly.
        let naive = |t: f32| {
            let (sx, sy) = compute_scroll_offset(&bg, t);
            dot_pulse(sx_screen - sx, sy_screen - sy, t, (0.0, 0.0))
        };
        assert!(
            (naive(after) - naive(before)).abs() > 0.05,
            "canvas-local phase should step at a wrap — if it no longer does, \
             this test has stopped proving anything"
        );
    }

    /// The pulse must be untouched before the first wrap, so the fix cannot
    /// change how any existing scenario's opening seconds look.
    #[test]
    fn dot_pulse_matches_the_original_formula_with_no_wrap_yet() {
        let bg = grid_bg(ScrollDirection::Right, 60.0);
        for t in [0.0_f32, 0.1, 0.5] {
            let (sx, sy) = compute_scroll_offset(&bg, t);
            let (rx, ry) = raw_scroll_offset(&bg, t);
            assert_eq!(
                (rx - sx, ry - sy),
                (0.0, 0.0),
                "t={t}: no whole period wrapped away yet"
            );
            let expected = (40.0_f32 * 0.01 + 20.0 * 0.01 + t * 2.0).sin() * 0.3 + 0.7;
            assert_eq!(dot_pulse(40.0, 20.0, t, (rx - sx, ry - sy)), expected);
        }
    }

    #[test]
    fn scroll_offset_wraps_consistently_for_left_direction_too() {
        let bg = grid_bg(ScrollDirection::Left, 60.0);
        for t in [0.0f32, 3.0, 10.0] {
            let (dx, _dy) = compute_scroll_offset(&bg, t);
            assert!(
                (-40.0..=40.0).contains(&dx),
                "t={t}: dx={dx} must stay within one tile period"
            );
        }
    }
}

#[cfg(test)]
mod gradient_linear_space_tests {
    //! TDD tests for paint.md finding #7: the "linear color space
    //! interpolation" and "subdivided stops" banding mitigations were both
    //! inert (the render surfaces carry no Skia `ColorSpace`, so the
    //! `ColorsInSpace(..., Some(linear_cs))` tag was a silent no-op, and
    //! subdividing an already-sRGB-space lerp is a mathematical identity).
    //! Fix: `subdivide_gradient_stops` itself now interpolates in linear
    //! light and re-encodes to sRGB per generated stop.

    use super::*;
    use skia_safe::Color4f;

    #[test]
    fn subdivide_interpolates_in_linear_light_not_srgb_gamma() {
        // Black -> white: the true midpoint in *linear light* (0.5) encodes
        // back to sRGB as ~188, not the naive sRGB-byte midpoint of 127.
        let black = Color4f::new(0.0, 0.0, 0.0, 1.0);
        let white = Color4f::new(1.0, 1.0, 1.0, 1.0);
        let (colors, positions) = subdivide_gradient_stops(&[black, white], 1);
        assert_eq!(positions.len(), 3, "1 subdivision -> stops at 0, 0.5, 1.0");
        assert_eq!(positions[1], 0.5);
        let mid_255 = (colors[1].r * 255.0).round() as i32;
        assert!(
            (mid_255 - 188).abs() <= 3,
            "midpoint should be ~188 (linear-light average re-encoded to sRGB), got {mid_255}"
        );
    }

    #[test]
    fn subdivide_endpoints_are_exact() {
        let a = Color4f::new(0.2, 0.4, 0.6, 1.0);
        let b = Color4f::new(0.8, 0.1, 0.9, 1.0);
        let (colors, positions) = subdivide_gradient_stops(&[a, b], 16);
        assert_eq!(positions[0], 0.0);
        assert_eq!(*positions.last().unwrap(), 1.0);
        let first = colors[0];
        let last = *colors.last().unwrap();
        assert!((first.r - a.r).abs() < 1e-4, "first.r={}", first.r);
        assert!((first.g - a.g).abs() < 1e-4, "first.g={}", first.g);
        assert!((first.b - a.b).abs() < 1e-4, "first.b={}", first.b);
        assert!((last.r - b.r).abs() < 1e-4, "last.r={}", last.r);
        assert!((last.g - b.g).abs() < 1e-4, "last.g={}", last.g);
        assert!((last.b - b.b).abs() < 1e-4, "last.b={}", last.b);
    }

    #[test]
    fn subdivide_alpha_stays_linear_not_gamma_corrected() {
        // Alpha is not gamma-encoded — it must keep lerping plainly, unlike
        // RGB.
        let a = Color4f::new(0.0, 0.0, 0.0, 0.0);
        let b = Color4f::new(0.0, 0.0, 0.0, 1.0);
        let (colors, _positions) = subdivide_gradient_stops(&[a, b], 1);
        assert!(
            (colors[1].a - 0.5).abs() < 1e-4,
            "alpha midpoint should be a plain 0.5 lerp, got {}",
            colors[1].a
        );
    }
}
