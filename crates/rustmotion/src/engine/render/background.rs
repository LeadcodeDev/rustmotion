use skia_safe::{Canvas, ColorType, ImageInfo, Paint};

use crate::schema::{
    AnimatedBackground, BackgroundPreset, ConcentricCirclesConfig, GradientShiftConfig,
    GradientType, GridDotsConfig, GridLinesConfig, HaloConfig, HaloZone, HeropatternConfig,
    PixelDensityRamp, PixelGridConfig, PixelGridMotion, ScrollDirection,
};
use rustmotion_core::engine::renderer::{color4f_from_hex, paint_from_hex};

pub(super) fn draw_animated_background(
    canvas: &Canvas,
    bg: &AnimatedBackground,
    time: f32,
    width: f32,
    height: f32,
) {
    let (scroll_x, scroll_y) = compute_scroll_offset(bg, time);

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
            let (world_x, world_y, world_w, world_h) = world;
            canvas.save();
            canvas.translate((world_x - cam_x, world_y - cam_y));
            draw_bg_halo(canvas, cfg, bg.speed, time, world_w, world_h);
            canvas.restore();
        }
        _ => {
            let (spacing_x, spacing_y) = tile_spacing(&bg.preset);
            let offset_x = -(cam_x % spacing_x);
            let offset_y = -(cam_y % spacing_y);
            canvas.save();
            canvas.translate((offset_x, offset_y));
            draw_animated_background(
                canvas,
                bg,
                time,
                width + spacing_x * 2.0,
                height + spacing_y * 2.0,
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
    use skia_safe::{
        gradient::{self, Colors, Gradient},
        Point,
    };

    if cfg.colors.len() < 2 {
        return;
    }

    let base_colors: Vec<skia_safe::Color4f> =
        cfg.colors.iter().map(|c| color4f_from_hex(c)).collect();

    let sign = match direction {
        Some(ScrollDirection::Ccw) => -1.0,
        _ => 1.0,
    };
    let angle = (sign * speed * time) % 360.0;
    let rad = angle.to_radians();

    let (colors, positions) = subdivide_gradient_stops(&base_colors, 16);

    let shader = match cfg.gradient_type {
        GradientType::Linear => {
            let cx = width / 2.0;
            let cy = height / 2.0;
            let half_diag = (width.powi(2) + height.powi(2)).sqrt() / 2.0;
            let start = Point::new(cx - rad.cos() * half_diag, cy - rad.sin() * half_diag);
            let end = Point::new(cx + rad.cos() * half_diag, cy + rad.sin() * half_diag);
            let gradient_colors = Colors::new(
                &colors,
                Some(&positions[..]),
                skia_safe::TileMode::Clamp,
                None,
            );
            let gradient = Gradient::new(gradient_colors, gradient::Interpolation::default());
            gradient::shaders::linear_gradient((start, end), &gradient, None)
        }
        GradientType::Radial => {
            let center = Point::new(width / 2.0, height / 2.0);
            let radius = width.max(height) / 2.0;
            let gradient_colors = Colors::new(
                &colors,
                Some(&positions[..]),
                skia_safe::TileMode::Clamp,
                None,
            );
            let gradient = Gradient::new(gradient_colors, gradient::Interpolation::default());
            gradient::shaders::radial_gradient((center, radius), &gradient, None)
        }
    };

    if let Some(shader) = shader {
        let mut paint = Paint::default();
        paint.set_shader(shader);
        paint.set_dither(true);
        canvas.draw_rect(skia_safe::Rect::from_wh(width, height), &paint);
    }
}

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
    out_colors.push(colors[n - 1]);
    out_pos.push(1.0);

    (out_colors, out_pos)
}

fn lerp_srgb_channel(a: f32, b: f32, t: f32) -> f32 {
    let linear = srgb_to_linear(a) + (srgb_to_linear(b) - srgb_to_linear(a)) * t;
    linear_to_srgb(linear)
}

fn srgb_to_linear(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

fn draw_bg_halo(canvas: &Canvas, cfg: &HaloConfig, speed: f32, time: f32, width: f32, height: f32) {
    for (i, zone) in cfg.zones.iter().enumerate() {
        let cx = zone.x * width;
        let cy = zone.y * height;
        let base_radius = zone.radius * width.max(height);
        let phase =
            (zone.x * 17.3 + zone.y * 31.7 + i as f32 * 0.73).fract() * std::f32::consts::TAU;
        const BREATH_RATE: f32 = 0.02;
        let freq = speed * BREATH_RATE * (0.7 + (zone.x * 13.1 + zone.y * 7.9).fract() * 0.6);
        let breath = 1.0 + 0.15 * (time * freq + phase).sin();
        let radius = base_radius * breath;

        let mut color = color4f_from_hex(&zone.color);
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
        let alpha = 1.0 - (r / max_radius).clamp(0.0, 1.0);
        paint.set_alpha_f(alpha * 0.3);
        canvas.draw_circle((cx, cy), r, &paint);
        r += spacing;
    }
}

fn dot_pulse(x: f32, y: f32, time: f32, phase_origin: (f32, f32)) -> f32 {
    let wx = x - phase_origin.0;
    let wy = y - phase_origin.1;
    (wx * 0.01 + wy * 0.01 + time * 2.0).sin() * 0.3 + 0.7
}

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

    let mut y = -spacing;
    while y < height + spacing {
        let mut x = -spacing;
        while x < width + spacing {
            let phase = dot_pulse(x, y, time, phase_origin);
            let r = dot_radius * phase;
            paint.set_alpha_f(phase * 0.4);
            canvas.draw_circle((x, y), r, &paint);
            x += spacing;
        }
        y += spacing;
    }
}

fn draw_bg_grid_lines(canvas: &Canvas, cfg: &GridLinesConfig, width: f32, height: f32) {
    let cell = cfg.cell.max(4.0);
    let mut minor = paint_from_hex(&cfg.color);
    minor.set_anti_alias(false);
    minor.set_stroke_width(cfg.weight.max(0.5));
    minor.set_style(skia_safe::PaintStyle::Stroke);
    let mut major = minor.clone();
    major.set_stroke_width(cfg.major_weight.max(0.5));

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
        PixelDensityRamp::Radial => {
            let (dx, dy) = (fx - 0.5, fy - 0.5);
            (1.0 - (dx * dx + dy * dy).sqrt() * 2.0).clamp(0.0, 1.0)
        }
        PixelDensityRamp::Edges => {
            let (dx, dy) = (fx - 0.5, fy - 0.5);
            ((dx * dx + dy * dy).sqrt() * 2.0).clamp(0.0, 1.0)
        }
    }
}

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
                *paint = paint_from_hex(&cfg.colors[idx]);
                paint.set_anti_alias(cfg.radius > 0.0);
            }
        }
    }
}

fn canonical_hero_color(color: &str) -> String {
    let c = color4f_from_hex(color);
    format!(
        "#{:02X}{:02X}{:02X}{:02X}",
        (c.r.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.g.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.b.clamp(0.0, 1.0) * 255.0).round() as u8,
        (c.a.clamp(0.0, 1.0) * 255.0).round() as u8,
    )
}

fn heropattern_svg_options() -> usvg::Options<'static> {
    usvg::Options {
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_string: Box::new(|_, _| None),
            ..usvg::ImageHrefResolver::default()
        },
        ..usvg::Options::default()
    }
}

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

    let svg_content = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="{}" viewBox="0 0 {} {}">{}</svg>"#,
        def.width,
        def.height,
        def.width,
        def.height,
        def.svg_paths
            .replace("{{color}}", &canonical_hero_color(&cfg.color))
            .replace("{{opacity}}", &cfg.opacity.to_string()),
    );

    let opt = heropattern_svg_options();
    let Ok(tree) = usvg::Tree::from_data(svg_content.as_bytes(), &opt) else {
        eprintln!(
            "warning: heropattern '{}' (colour '{}') failed to parse as SVG — background not rendered",
            cfg.pattern, cfg.color
        );
        return;
    };

    let (pw, ph) = heropattern_raster_size(def.width, def.height, cfg.scale);
    let Some(mut pixmap) = tiny_skia::Pixmap::new(pw, ph) else {
        return;
    };
    let render_scale_x = pw as f32 / def.width;
    let render_scale_y = ph as f32 / def.height;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(render_scale_x, render_scale_y),
        &mut pixmap.as_mut(),
    );

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

    let Some(shader) = tile_image.to_shader(
        (skia_safe::TileMode::Repeat, skia_safe::TileMode::Repeat),
        skia_safe::SamplingOptions::new(skia_safe::FilterMode::Linear, skia_safe::MipmapMode::None),
        None,
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

fn heropattern_raster_size(width: f32, height: f32, scale: f32) -> (u32, u32) {
    const MAX_TILE_PX: f32 = 4096.0;
    let pw = (width * scale).ceil().clamp(1.0, MAX_TILE_PX) as u32;
    let ph = (height * scale).ceil().clamp(1.0, MAX_TILE_PX) as u32;
    (pw, ph)
}

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

fn tile_spacing(preset: &BackgroundPreset) -> (f32, f32) {
    match preset {
        BackgroundPreset::GridDots(cfg) => {
            let s = cfg.spacing.max(20.0);
            (s, s)
        }
        BackgroundPreset::GridLines(cfg) => {
            let s = cfg.cell.max(4.0);
            (s, s)
        }
        BackgroundPreset::PixelGrid(cfg) => {
            let s = cfg.spacing.max(cfg.size.max(1.0));
            (s, s)
        }
        BackgroundPreset::ConcentricCircles(cfg) => {
            let s = cfg.spacing.max(20.0);
            (s, s)
        }
        BackgroundPreset::Heropattern(cfg) => {
            match crate::engine::heropatterns::find_pattern(&cfg.pattern) {
                Some(d) => (
                    period_floor(d.width * cfg.scale, 20.0),
                    period_floor(d.height * cfg.scale, 20.0),
                ),
                None => (60.0, 60.0),
            }
        }
        _ => (60.0, 60.0),
    }
}

fn period_floor(period: f32, floor: f32) -> f32 {
    if period <= 0.0 {
        floor
    } else if period >= floor {
        period
    } else {
        period * (floor / period).ceil()
    }
}

pub(super) fn compute_scroll_offset(bg: &AnimatedBackground, time: f32) -> (f32, f32) {
    let (raw_x, raw_y) = raw_scroll_offset(bg, time);
    let (spacing_x, spacing_y) = tile_spacing(&bg.preset);
    (raw_x % spacing_x, raw_y % spacing_y)
}

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
        _ => (0.0, 0.0),
    };
    (dx * speed * time, dy * speed * time)
}

#[cfg(test)]
mod halo_opacity_tests {
    use crate::encode::video::{build_frame_tasks, render_frame_task, FrameTask};
    use crate::loader::load_scenario_from_source;

    fn render_first_frame(json: &str) -> Vec<u8> {
        let scenario = load_scenario_from_source(None, Some(json)).expect("load");
        let tasks = build_frame_tasks(&scenario);
        let task = tasks
            .iter()
            .find(|t| matches!(t, FrameTask::Normal { .. }))
            .expect("normal task");
        render_frame_task(&scenario.video, &scenario, task).expect("render")
    }

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
        assert_eq!(r1, 255);
        assert!(
            (r05 as i32 - 128).abs() <= 2,
            "0.5 opacity center was {r05}"
        );
        assert!((r02 as i32 - 51).abs() <= 2, "0.2 opacity center was {r02}");
    }

    #[test]
    fn opacity_multiplies_the_colors_own_hex_alpha() {
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
        assert_eq!(
            (r, g, b),
            (0, 0, 0),
            "negative opacity must clamp to fully transparent"
        );
    }
}

#[cfg(test)]
mod scroll_offset_wrap_tests {

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
        let bg = grid_bg(ScrollDirection::Right, 60.0);
        assert_eq!(compute_scroll_offset(&bg, 0.0), (0.0, 0.0));
    }

    #[test]
    fn scroll_offset_zero_speed_is_still_a_pure_noop() {
        let bg = grid_bg(ScrollDirection::Right, 0.0);
        assert_eq!(compute_scroll_offset(&bg, 5.0), (0.0, 0.0));
    }

    #[test]
    fn dot_pulse_is_continuous_across_a_wrap() {
        let bg = grid_bg(ScrollDirection::Right, 60.0);
        let (before, after) = (0.6666_f32, 0.6667_f32);
        assert!(
            compute_scroll_offset(&bg, before).0 > compute_scroll_offset(&bg, after).0,
            "test setup: these two instants must straddle a wrap"
        );

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

    use super::*;
    use skia_safe::Color4f;

    #[test]
    fn subdivide_interpolates_in_linear_light_not_srgb_gamma() {
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

    #[test]
    fn the_seed_changes_which_cells_are_drawn() {
        let same_seed: Vec<f32> = (0..40).map(|i| cell_hash(i, 0, 7, 0)).collect();
        let other_seed: Vec<f32> = (0..40).map(|i| cell_hash(i, 0, 8, 0)).collect();
        assert_ne!(same_seed, other_seed);
    }

    #[test]
    fn neighbouring_cells_are_uncorrelated() {
        let drawn = |c: i32, r: i32| cell_hash(c, r, 7, 0) < 0.5;
        let matches = (0..30)
            .flat_map(|c| (0..30).map(move |r| (c, r)))
            .filter(|&(c, r)| drawn(c, r) == drawn(c + 1, r + 1))
            .count();
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
        assert!(ramp_at(PixelDensityRamp::Radial, 50.0, 50.0, w, h) > 0.99);
        assert_eq!(ramp_at(PixelDensityRamp::Radial, 0.0, 0.0, w, h), 0.0);
        assert_eq!(ramp_at(PixelDensityRamp::Edges, 50.0, 50.0, w, h), 0.0);
        assert!(ramp_at(PixelDensityRamp::Edges, 0.0, 50.0, w, h) > 0.99);
        assert!(ramp_at(PixelDensityRamp::Edges, 100.0, 50.0, w, h) > 0.99);
        for (x, y) in [(0.0, 0.0), (25.0, 60.0), (100.0, 50.0)] {
            let r = ramp_at(PixelDensityRamp::Radial, x, y, w, h);
            let e = ramp_at(PixelDensityRamp::Edges, x, y, w, h);
            assert!((r + e - 1.0).abs() < 1e-6, "at ({x},{y}): {r} + {e} != 1");
        }

        for x in [0.0, 50.0, 100.0] {
            assert_eq!(ramp_at(PixelDensityRamp::None, x, 0.0, w, h), 1.0);
        }
    }

    #[test]
    fn spacing_never_goes_below_the_cell_size() {
        let mut c = cfg();
        c.size = 40.0;
        c.spacing = 8.0;
        assert_eq!(tile_spacing(&BackgroundPreset::PixelGrid(c)), (40.0, 40.0));
    }

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
        let buf = render(base(0.0));
        assert_eq!(buf.len(), (W * H * 4) as usize, "it still produced a frame");
    }
}

#[cfg(test)]
mod heropattern_period_tests {

    use super::*;
    use crate::schema::HeropatternConfig;

    fn hero_bg(pattern: &str, direction: ScrollDirection, speed: f32) -> AnimatedBackground {
        AnimatedBackground {
            preset: BackgroundPreset::Heropattern(HeropatternConfig {
                pattern: pattern.to_string(),
                color: "#FFFFFF".to_string(),
                opacity: 0.1,
                scale: 1.0,
            }),
            x: 0.0,
            y: 0.0,
            speed,
            direction: Some(direction),
        }
    }

    #[test]
    fn tile_spacing_is_per_axis_for_a_non_square_pattern() {
        let bg = hero_bg("aztec", ScrollDirection::Down, 60.0);
        let (spacing_x, spacing_y) = tile_spacing(&bg.preset);
        assert_eq!(spacing_x, 32.0, "x period must be the pattern's own width");
        assert_eq!(
            spacing_y, 64.0,
            "y period must be the pattern's own height, not its width"
        );
    }

    #[test]
    fn vertical_scroll_does_not_wrap_at_half_the_tile_height() {
        let bg = hero_bg("aztec", ScrollDirection::Down, 60.0);
        let t = 48.0 / 60.0;
        let (_dx, dy) = compute_scroll_offset(&bg, t);
        assert!(
            (dy - 48.0).abs() < 1e-2,
            "48px of vertical travel is under one tile height (64px) and must not wrap yet, got dy={dy}"
        );
    }

    #[test]
    fn narrow_pattern_wraps_on_a_whole_multiple_of_its_own_period() {
        let bg = hero_bg("bamboo", ScrollDirection::Right, 60.0);
        let (spacing_x, _spacing_y) = tile_spacing(&bg.preset);
        assert_eq!(
            spacing_x % 16.0,
            0.0,
            "the clamped period must stay a whole multiple of the pattern's own 16px width, got {spacing_x}"
        );
        assert!(spacing_x >= 20.0);
    }
}

#[cfg(test)]
mod heropattern_raster_tests {

    use super::*;

    #[test]
    fn raster_size_scales_with_cfg_scale_not_pinned_to_1x() {
        let (pw, ph) = heropattern_raster_size(32.0, 64.0, 4.0);
        assert_eq!(
            (pw, ph),
            (128, 256),
            "the pixmap must be sized for the scaled tile, not the pattern's native 32x64"
        );
    }

    #[test]
    fn raster_size_matches_the_pattern_exactly_at_scale_1() {
        assert_eq!(heropattern_raster_size(32.0, 64.0, 1.0), (32, 64));
    }

    #[test]
    fn raster_size_is_clamped_for_an_unbounded_scale() {
        let (pw, ph) = heropattern_raster_size(32.0, 64.0, 100_000.0);
        assert!(
            pw <= 4096 && ph <= 4096,
            "an extreme scale must not attempt an unbounded pixmap allocation, got {pw}x{ph}"
        );
    }

    #[test]
    fn draw_bg_heropattern_does_not_panic_at_an_extreme_scale() {
        let mut surface = skia_safe::surfaces::raster_n32_premul((64, 64)).expect("surface");
        let cfg = HeropatternConfig {
            pattern: "aztec".to_string(),
            color: "#FFFFFF".to_string(),
            opacity: 0.1,
            scale: 100_000.0,
        };
        draw_bg_heropattern(surface.canvas(), &cfg, 0.0, 64.0, 64.0);
    }
}

#[cfg(test)]
mod heropattern_svg_injection_tests {

    use super::*;

    #[test]
    fn a_colour_containing_a_double_quote_cannot_inject_markup() {
        let payload = r#""/><image href="/etc/passwd"/><rect fill=""#;
        let sanitized = canonical_hero_color(payload);
        assert!(!sanitized.contains('"'), "got: {sanitized}");
        assert!(!sanitized.contains('<'), "got: {sanitized}");
        assert!(!sanitized.contains('&'), "got: {sanitized}");
        assert_eq!(
            sanitized.len(),
            9,
            "expected '#' + 8 hex digits, got: {sanitized}"
        );
        assert!(sanitized
            .strip_prefix('#')
            .unwrap()
            .chars()
            .all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn a_valid_colour_round_trips_case_normalised() {
        assert_eq!(canonical_hero_color("#1e3a8a55"), "#1E3A8A55");
    }

    #[test]
    fn the_svg_options_never_read_a_file_from_disk() {
        let scratch_path = std::env::temp_dir().join(format!(
            "rustmotion-heropattern-injection-probe-{}.svg",
            std::process::id()
        ));
        std::fs::write(
            &scratch_path,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="1" height="1"></svg>"#,
        )
        .expect("scratch SVG written");

        let opt = heropattern_svg_options();
        let resolved =
            (opt.image_href_resolver.resolve_string)(scratch_path.to_str().unwrap(), &opt);

        let _ = std::fs::remove_file(&scratch_path);

        assert!(
            resolved.is_none(),
            "the string resolver must be neutralised, not read a real SVG file from disk"
        );
    }

    #[test]
    fn draw_bg_heropattern_survives_an_injection_attempt_without_panicking() {
        let mut surface = skia_safe::surfaces::raster_n32_premul((32, 32)).expect("surface");
        let cfg = HeropatternConfig {
            pattern: "aztec".to_string(),
            color: r#""/><image href="/etc/passwd"/><rect fill=""#.to_string(),
            opacity: 1.0,
            scale: 1.0,
        };
        draw_bg_heropattern(surface.canvas(), &cfg, 0.0, 32.0, 32.0);
    }
}
