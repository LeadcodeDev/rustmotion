use skia_safe::{Canvas, ColorType, ImageInfo, Paint};

use crate::schema::{
    AnimatedBackground, BackgroundPreset, ConcentricCirclesConfig, GradientShiftConfig,
    GradientType, GridDotsConfig, GridLinesConfig, HaloConfig, HaloZone, HeropatternConfig,
    PixelDensityRamp, PixelGridConfig, PixelGridMotion, ScrollDirection,
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
        BackgroundPreset::GridLines(cfg) => draw_bg_grid_lines(canvas, cfg, width, height),
        BackgroundPreset::ConcentricCircles(cfg) => {
            draw_bg_concentric_circles(canvas, cfg, bg.speed, time, width, height)
        }
        BackgroundPreset::Halo(cfg) => draw_bg_halo(canvas, cfg, bg.speed, time, width, height),
        BackgroundPreset::PixelGrid(cfg) => {
            draw_bg_pixel_grid(canvas, cfg, bg.speed, time, width, height)
        }
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
    world: (f32, f32, f32, f32),
) {
    match &bg.preset {
        BackgroundPreset::Halo(cfg) => {
            // `HaloZone`'s x/y/radius are fractions of the surface it is painted
            // on. In a slide view that is the viewport; here it is the world the
            // camera travels, so it has to be the *actual* extent
            // (`WorldTimeline::world_extent`). It used to be `viewport * 5.0`,
            // a constant unrelated to the scenes' own positions: the same
            // `radius: 0.55` that reads as a half-screen glow in a slide became
            // a five-screen wash, and calibrating one was trial and error.
            let (world_x, world_y, world_w, world_h) = world;
            canvas.save();
            canvas.translate((world_x - cam_x, world_y - cam_y));
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

// The non-deprecated gradient API moves TileMode out of the signature; the
// transposition is tracked in #215, and doing it here without pixel tests
// would change rendering silently.
#[allow(deprecated)]
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

/// A ruled grid of horizontal and vertical lines.
///
/// Unlike `grid_dots`, nothing here pulses: a grid is structure behind the
/// content, and structure that breathes competes with what sits on it. The
/// motion, if any, comes from the shared `speed`/`direction` scroll applied by
/// the caller's canvas translate.
fn draw_bg_grid_lines(canvas: &Canvas, cfg: &GridLinesConfig, width: f32, height: f32) {
    let cell = cfg.cell.max(4.0);
    let mut minor = paint_from_hex(&cfg.color);
    minor.set_anti_alias(false); // hairlines stay crisp unblurred
    minor.set_stroke_width(cfg.weight.max(0.5));
    minor.set_style(skia_safe::PaintStyle::Stroke);
    let mut major = minor.clone();
    major.set_stroke_width(cfg.major_weight.max(0.5));

    // One cell of overscan on every side: the caller wraps the scroll offset
    // into `(-cell, cell)`, so without the margin a scrolled grid leaves a
    // blank band on the leading edge (the same trap `draw_bg_grid_dots`
    // documents on its own loop).
    let pick = |index: i32| -> &Paint {
        if cfg.major_every > 0 && index.rem_euclid(cfg.major_every as i32) == 0 {
            &major
        } else {
            &minor
        }
    };

    let cols = ((width / cell).ceil() as i32) + 2;
    for i in -1..cols {
        let x = i as f32 * cell;
        canvas.draw_line((x, -cell), (x, height + cell), pick(i));
    }
    let rows = ((height / cell).ceil() as i32) + 2;
    for j in -1..rows {
        let y = j as f32 * cell;
        canvas.draw_line((-cell, y), (width + cell, y), pick(j));
    }
}

/// Deterministic 0..1 value for a cell.
///
/// A hash of the coordinates, not a random number generator: the same cell
/// must resolve the same way on every frame, or the field boils instead of
/// holding still. Same reason `seed` is part of the input rather than process
/// state — two renders of one scenario have to match.
fn cell_hash(col: i32, row: i32, seed: u32, salt: u32) -> f32 {
    let mut h = seed
        .wrapping_mul(0x9E37_79B9)
        .wrapping_add((col as u32).wrapping_mul(0x85EB_CA6B))
        .wrapping_add((row as u32).wrapping_mul(0xC2B2_AE35))
        .wrapping_add(salt.wrapping_mul(0x27D4_EB2F));
    h ^= h >> 15;
    h = h.wrapping_mul(0x2545_F491);
    h ^= h >> 13;
    (h & 0x00FF_FFFF) as f32 / 0x0100_0000 as f32
}

/// How much of the field is filled at this point, before the cell's own draw.
fn ramp_at(ramp: PixelDensityRamp, x: f32, y: f32, width: f32, height: f32) -> f32 {
    let fx = if width > 0.0 {
        (x / width).clamp(0.0, 1.0)
    } else {
        0.5
    };
    let fy = if height > 0.0 {
        (y / height).clamp(0.0, 1.0)
    } else {
        0.5
    };
    match ramp {
        PixelDensityRamp::None => 1.0,
        PixelDensityRamp::Right => fx,
        PixelDensityRamp::Left => 1.0 - fx,
        PixelDensityRamp::Bottom => fy,
        PixelDensityRamp::Top => 1.0 - fy,
        // Full at the centre, nothing at the corners.
        PixelDensityRamp::Radial => {
            let (dx, dy) = (fx - 0.5, fy - 0.5);
            (1.0 - (dx * dx + dy * dy).sqrt() * 2.0).clamp(0.0, 1.0)
        }
        // The inverse: a vignette that leaves the middle clear.
        PixelDensityRamp::Edges => {
            let (dx, dy) = (fx - 0.5, fy - 0.5);
            ((dx * dx + dy * dy).sqrt() * 2.0).clamp(0.0, 1.0)
        }
    }
}

/// A lattice of square cells: the pixel-tile texture.
///
/// Occupancy is decided per cell by a hash against `density * ramp`, so the
/// pattern is stable across frames and reproducible across renders. Colours
/// alternate by `(col + row)`, which turns two colours at `density: 1.0` into
/// a checkerboard and one colour below 1.0 into a scattered tile field.
fn draw_bg_pixel_grid(
    canvas: &Canvas,
    cfg: &PixelGridConfig,
    speed: f32,
    time: f32,
    width: f32,
    height: f32,
) {
    if cfg.colors.is_empty() {
        return;
    }
    let size = cfg.size.max(1.0);
    // Cells must not overlap: a spacing under `size` would draw a solid sheet
    // and silently lose the lattice the preset exists for.
    let spacing = cfg.spacing.max(size);
    let density = cfg.density.clamp(0.0, 1.0);
    let t = time * speed.max(0.0);

    let mut paints: Vec<_> = cfg
        .colors
        .iter()
        .map(|c| {
            let mut p = paint_from_hex(c);
            p.set_anti_alias(cfg.radius > 0.0);
            p
        })
        .collect();

    let cols = (width / spacing).ceil() as i32 + 1;
    let rows = (height / spacing).ceil() as i32 + 1;

    for row in 0..rows {
        for col in 0..cols {
            let x = col as f32 * spacing;
            let y = row as f32 * spacing;

            let mut threshold = density * ramp_at(cfg.density_ramp, x, y, width, height);
            if cfg.motion == PixelGridMotion::Sweep {
                // A band of extra density crossing the field, wrapped so it
                // never runs off and leaves the texture flat.
                let head = (t * 0.25).fract();
                let d = ((x / width.max(1.0)) - head).abs().min(1.0);
                threshold += (0.35 - d).max(0.0);
            }

            if cell_hash(col, row, cfg.seed, 0) >= threshold.clamp(0.0, 1.0) {
                continue;
            }

            let idx = ((col + row).rem_euclid(paints.len() as i32)) as usize;
            let paint = &mut paints[idx];

            if cfg.motion == PixelGridMotion::Twinkle {
                // Full 0 → 1 → 0, not a partial dip: a cell that only fades to
                // 10 % still reads as a permanent dot, so the lattice looks
                // fixed and merely dimmer. Each cell gets its own phase from
                // its own hash, or the whole field blinks in unison.
                let phase = cell_hash(col, row, cfg.seed, 1) * std::f32::consts::TAU;
                let a = 0.5 + 0.5 * (t * 1.6 + phase).sin();
                paint.set_alpha_f(paint.alpha_f() * a);
            }

            let rect = skia_safe::Rect::from_xywh(x, y, size, size);
            if cfg.radius > 0.0 {
                let rr = skia_safe::RRect::new_rect_xy(rect, cfg.radius, cfg.radius);
                canvas.draw_rrect(rr, paint);
            } else {
                canvas.draw_rect(rect, paint);
            }

            if cfg.motion == PixelGridMotion::Twinkle {
                // `paint` is reused by the next cell that picks this colour.
                *paint = paint_from_hex(&cfg.colors[idx]);
                paint.set_anti_alias(cfg.radius > 0.0);
            }
        }
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
        BackgroundPreset::GridLines(cfg) => cfg.cell.max(4.0),
        // The lattice repeats on `spacing`, so the world-view camera can wrap
        // on it and the tiling stays seamless as the camera pans.
        BackgroundPreset::PixelGrid(cfg) => cfg.spacing.max(cfg.size.max(1.0)),
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

#[cfg(test)]
mod pixel_grid_tests {
    use super::*;

    fn cfg() -> PixelGridConfig {
        PixelGridConfig {
            colors: vec!["#FFFFFF".to_string()],
            size: 10.0,
            spacing: 24.0,
            density: 0.6,
            density_ramp: PixelDensityRamp::None,
            radius: 0.0,
            seed: 7,
            motion: PixelGridMotion::None,
        }
    }

    /// The pattern must be a function of the cell, not of when it is drawn:
    /// a per-frame random makes the whole field boil.
    #[test]
    fn a_cell_resolves_the_same_way_every_time() {
        let a = cell_hash(3, 5, 7, 0);
        let b = cell_hash(3, 5, 7, 0);
        assert_eq!(a, b);
        assert!(
            (0.0..1.0).contains(&a),
            "hash must be a 0..1 fraction, got {a}"
        );
    }

    /// …and two seeds must actually scatter differently, or `seed` is a lie.
    #[test]
    fn the_seed_changes_which_cells_are_drawn() {
        let same_seed: Vec<f32> = (0..40).map(|i| cell_hash(i, 0, 7, 0)).collect();
        let other_seed: Vec<f32> = (0..40).map(|i| cell_hash(i, 0, 8, 0)).collect();
        assert_ne!(same_seed, other_seed);
    }

    /// Neighbouring cells must not correlate — a hash that walks in step with
    /// the coordinate draws diagonal stripes instead of a scatter.
    #[test]
    fn neighbouring_cells_are_uncorrelated() {
        let drawn = |c: i32, r: i32| cell_hash(c, r, 7, 0) < 0.5;
        let matches = (0..30)
            .flat_map(|c| (0..30).map(move |r| (c, r)))
            .filter(|&(c, r)| drawn(c, r) == drawn(c + 1, r + 1))
            .count();
        // 900 cells; a stripe pattern would agree ~100 % of the time.
        assert!(
            (300..600).contains(&matches),
            "diagonal neighbours agree {matches}/900 times — that is a pattern, not a scatter"
        );
    }

    #[test]
    fn density_ramps_run_the_direction_they_name() {
        let (w, h) = (100.0, 100.0);
        assert!(ramp_at(PixelDensityRamp::Right, 90.0, 50.0, w, h) > 0.8);
        assert!(ramp_at(PixelDensityRamp::Right, 10.0, 50.0, w, h) < 0.2);
        assert!(ramp_at(PixelDensityRamp::Left, 10.0, 50.0, w, h) > 0.8);
        assert!(ramp_at(PixelDensityRamp::Bottom, 90.0, 90.0, w, h) > 0.8);
        assert!(ramp_at(PixelDensityRamp::Top, 50.0, 10.0, w, h) > 0.8);
        // Radial is full at the centre and gone at the corners.
        assert!(ramp_at(PixelDensityRamp::Radial, 50.0, 50.0, w, h) > 0.99);
        assert_eq!(ramp_at(PixelDensityRamp::Radial, 0.0, 0.0, w, h), 0.0);
        // `Edges` is the vignette: clear in the middle, heavy at the border.
        assert_eq!(ramp_at(PixelDensityRamp::Edges, 50.0, 50.0, w, h), 0.0);
        assert!(ramp_at(PixelDensityRamp::Edges, 0.0, 50.0, w, h) > 0.99);
        assert!(ramp_at(PixelDensityRamp::Edges, 100.0, 50.0, w, h) > 0.99);
        // …and the exact inverse of `Radial`, which is the point of having both.
        for (x, y) in [(0.0, 0.0), (25.0, 60.0), (100.0, 50.0)] {
            let r = ramp_at(PixelDensityRamp::Radial, x, y, w, h);
            let e = ramp_at(PixelDensityRamp::Edges, x, y, w, h);
            assert!((r + e - 1.0).abs() < 1e-6, "at ({x},{y}): {r} + {e} != 1");
        }

        // `None` is uniform: the same everywhere, including the edges.
        for x in [0.0, 50.0, 100.0] {
            assert_eq!(ramp_at(PixelDensityRamp::None, x, 0.0, w, h), 1.0);
        }
    }

    /// `spacing` below `size` would draw a solid sheet and lose the lattice
    /// the preset exists for, so it is clamped rather than honoured.
    #[test]
    fn spacing_never_goes_below_the_cell_size() {
        let mut c = cfg();
        c.size = 40.0;
        c.spacing = 8.0;
        assert_eq!(tile_spacing(&BackgroundPreset::PixelGrid(c)), 40.0);
    }

    /// `twinkle` has to reach both ends. A cell that only dips to 10 % still
    /// reads as a permanent dot: the field looks fixed, just dimmer.
    #[test]
    fn twinkle_spans_the_whole_opacity_range() {
        let phase = 0.0f32;
        let alpha = |t: f32| 0.5 + 0.5 * (t * 1.6 + phase).sin();
        let samples: Vec<f32> = (0..400).map(|i| alpha(i as f32 * 0.01)).collect();
        let lo = samples.iter().cloned().fold(f32::MAX, f32::min);
        let hi = samples.iter().cloned().fold(f32::MIN, f32::max);
        assert!(lo < 0.01, "must fade all the way out, floor was {lo}");
        assert!(hi > 0.99, "must come all the way back, ceiling was {hi}");
    }

    /// Degenerate configs must be inert, not panic: an empty palette has
    /// nothing to draw with, and a zero size no area to draw.
    #[test]
    fn degenerate_configs_draw_nothing_without_panicking() {
        let mut surface = skia_safe::surfaces::raster_n32_premul((32, 32)).expect("surface");
        let canvas = surface.canvas();

        let mut empty = cfg();
        empty.colors.clear();
        draw_bg_pixel_grid(canvas, &empty, 1.0, 0.0, 32.0, 32.0);

        let mut zero = cfg();
        zero.size = 0.0;
        zero.spacing = 0.0;
        draw_bg_pixel_grid(canvas, &zero, 1.0, 0.0, 32.0, 32.0);

        let mut over = cfg();
        over.density = 5.0;
        draw_bg_pixel_grid(canvas, &over, 1.0, 0.0, 32.0, 32.0);
    }
}

#[cfg(test)]
mod grid_lines_tests {
    //! `grid_lines` is the ruled counterpart of `grid_dots`: dots mark the
    //! intersections and read as texture, lines read as structure. The
    //! properties worth pinning are that the lines actually land on the cell
    //! pitch, that `major_every` thickens the right ones, and that the grid
    //! covers the frame edge to edge (the trap `grid_dots` already documents
    //! on its own loop: an asymmetric overscan leaves a blank leading band
    //! once the background scrolls).

    use super::*;
    use crate::schema::GridLinesConfig;

    const W: i32 = 200;
    const H: i32 = 120;

    fn render(cfg: GridLinesConfig) -> Vec<u8> {
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        draw_bg_grid_lines(surface.canvas(), &cfg, W as f32, H as f32);
        let snapshot = surface.image_snapshot();
        let info = ImageInfo::new(
            (W, H),
            ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut buf = vec![0u8; (W * H * 4) as usize];
        assert!(snapshot.read_pixels(
            &info,
            &mut buf,
            (W * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        ));
        buf
    }

    fn alpha_at(buf: &[u8], x: i32, y: i32) -> u8 {
        buf[((y * W + x) * 4) as usize + 3]
    }

    fn column_ink(buf: &[u8], x: i32) -> u32 {
        (0..H).map(|y| alpha_at(buf, x, y) as u32).sum()
    }

    fn base(cell: f32) -> GridLinesConfig {
        GridLinesConfig {
            color: "#FFFFFFFF".into(),
            cell,
            weight: 1.0,
            major_every: 0,
            major_weight: 2.0,
        }
    }

    #[test]
    fn lines_land_on_the_cell_pitch() {
        let buf = render(base(40.0));
        for x in [0, 40, 80, 120, 160] {
            assert!(
                column_ink(&buf, x) > 0,
                "a vertical line should sit at x={x} (one cell pitch apart)"
            );
        }
        // And a mid-cell column carries only where the *horizontal* lines
        // cross it — a few pixels, not a full column. Without that gap this
        // would be a wash, not a grid.
        for x in [20, 60, 100] {
            assert!(
                column_ink(&buf, x) * 10 < column_ink(&buf, 40),
                "x={x} sits mid-cell: it should only carry the horizontal crossings ({}) not a \
                 full line ({})",
                column_ink(&buf, x),
                column_ink(&buf, 40)
            );
        }
    }

    #[test]
    fn the_grid_reaches_all_four_edges() {
        let buf = render(base(40.0));
        // Corners: the horizontal line at y=0 and the vertical at x=0 both
        // pass through, and the far edges are covered by the overscan.
        assert!(alpha_at(&buf, 0, 0) > 0, "top-left corner is on the grid");
        assert!(
            (0..H).any(|y| alpha_at(&buf, W - 1, y) > 0),
            "the right edge must be reached by horizontal lines"
        );
        assert!(
            (0..W).any(|x| alpha_at(&buf, x, H - 1) > 0),
            "the bottom edge must be reached by vertical lines"
        );
    }

    #[test]
    fn major_every_thickens_only_the_major_lines() {
        let cfg = GridLinesConfig {
            major_every: 2,
            major_weight: 5.0,
            ..base(40.0)
        };
        let buf = render(cfg);
        // Index 2 (x=80) is major, index 1 (x=40) is not. Compare the ink in
        // a band around each: the thick line spills into its neighbours.
        let band =
            |centre: i32| -> u32 { (centre - 3..=centre + 3).map(|x| column_ink(&buf, x)).sum() };
        assert!(
            band(80) > band(40) * 2,
            "the major line at x=80 should be markedly heavier than the minor one at x=40 \
             (major={}, minor={})",
            band(80),
            band(40)
        );
    }

    #[test]
    fn a_zero_major_every_leaves_every_line_the_same_weight() {
        let buf = render(base(40.0));
        let band =
            |centre: i32| -> u32 { (centre - 3..=centre + 3).map(|x| column_ink(&buf, x)).sum() };
        assert_eq!(
            band(40),
            band(80),
            "with major_every: 0 no line is special-cased"
        );
    }

    #[test]
    fn a_degenerate_cell_does_not_hang_the_render() {
        // 0 would be an infinite loop's worth of lines; the painter clamps.
        let buf = render(base(0.0));
        assert_eq!(buf.len(), (W * H * 4) as usize, "it still produced a frame");
    }
}
