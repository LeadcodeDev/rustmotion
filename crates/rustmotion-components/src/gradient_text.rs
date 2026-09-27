use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::gradient::{self, Colors, Gradient};
use skia_safe::{Canvas, Color4f, Font, FontStyle, Point};

use rustmotion_core::css::style::{
    FontStyle as CssFontStyle, FontWeight as CssFontWeight, FontWeightKw,
    TextAlign as CssTextAlign, WhiteSpace as CssWhiteSpace,
};
use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{
    draw_text_with_fallback, emoji_typeface, measure_text_with_fallback, paint_from_hex,
    parse_hex_color, typeface_with_fallback, wrap_text_with_tracking,
};
use rustmotion_core::schema::{TextAlign, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

fn default_colors() -> Vec<String> {
    vec!["#3B82F6".to_string(), "#8B5CF6".to_string()]
}

fn default_angle() -> f32 {
    90.0
}

fn default_speed() -> f32 {
    0.5
}

/// An explicit color stop on a `gradient_text` ramp.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GradientTextStop {
    /// Hex color at this stop, e.g. `"#7C3AED"`.
    pub color: String,
    /// Position along the gradient line, from `0.0` (first stop) to `1.0` (last).
    pub position: f32,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GradientText {
    pub content: String,
    #[serde(default = "default_colors")]
    pub colors: Vec<String>,
    /// Explicit stop positions along the ramp. When omitted, `colors` are spaced evenly.
    #[serde(default)]
    pub stops: Option<Vec<GradientTextStop>>,
    #[serde(default = "default_angle")]
    pub angle: f32,
    #[serde(default)]
    pub animate_angle: bool,
    #[serde(default = "default_speed")]
    pub speed: f32,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(GradientText {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl GradientText {
    fn resolve_typeface(&self) -> Option<skia_safe::Typeface> {
        let font_family = self.style.font_family_or("Inter");

        let slant = match self.style.font_style {
            Some(CssFontStyle::Italic) => skia_safe::font_style::Slant::Italic,
            Some(CssFontStyle::Oblique) => skia_safe::font_style::Slant::Oblique,
            _ => skia_safe::font_style::Slant::Upright,
        };
        let weight = match &self.style.font_weight {
            Some(CssFontWeight::Keyword(FontWeightKw::Bold | FontWeightKw::Bolder)) => {
                skia_safe::font_style::Weight::BOLD
            }
            Some(CssFontWeight::Number(n)) => skia_safe::font_style::Weight::from(*n as i32),
            _ => skia_safe::font_style::Weight::NORMAL,
        };
        let skia_style = FontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant);

        typeface_with_fallback(font_family, skia_style).ok()
    }
}

impl GradientText {
    fn paint(
        &self,
        canvas: &Canvas,
        layout_width: f32,
        content_height: Option<f32>,
        time: f64,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let has_stops = self.stops.as_ref().is_some_and(|s| !s.is_empty());
        if self.content.is_empty() || (self.colors.is_empty() && !has_stops) {
            return;
        }

        let base_ctx = crate::intrinsic::font_size_ctx(
            ctx.video_width as f32,
            ctx.video_height as f32,
            layout_width.max(0.0),
        );
        let mut font_size = self.style.font_size_px_ctx(&base_ctx, 48.0);
        let Some(typeface) = self.resolve_typeface() else {
            return;
        };
        let type_ctx = rustmotion_core::css::units::LengthContext {
            font_size,
            ..base_ctx
        };
        let mut line_height_val = self.style.line_height_for_ctx(font_size, &type_ctx);
        let mut letter_spacing = self.style.letter_spacing_px_ctx(&type_ctx);

        let nowrap = matches!(
            self.style.white_space,
            Some(CssWhiteSpace::Nowrap | CssWhiteSpace::Pre)
        );
        let box_width = (layout_width.is_finite() && layout_width > 0.0).then_some(layout_width);
        let wrap_at = if nowrap { None } else { box_width };

        if matches!(self.style.text_autofit, Some(true)) {
            let declared_height = content_height.filter(|h| *h > 0.0 && h.is_finite());
            let (fs, ls, lh) = crate::intrinsic::resolve_text_autofit(
                &self.content,
                &typeface,
                font_size,
                letter_spacing,
                line_height_val,
                !nowrap,
                box_width,
                declared_height,
            );
            font_size = fs;
            letter_spacing = ls;
            line_height_val = lh;
        }

        let font = Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));

        let lines =
            wrap_text_with_tracking(&self.content, &font, &emoji_font, wrap_at, letter_spacing);

        let (_, metrics) = font.metrics();
        let ascent = -metrics.ascent;
        let descent = metrics.descent;
        let text_w = lines
            .iter()
            .map(|l| measure_text_with_fallback(l, &font, &emoji_font, letter_spacing))
            .fold(0.0f32, f32::max);
        let text_h = (lines.len().max(1) - 1) as f32 * line_height_val + ascent + descent;

        let align_width = if layout_width.is_finite() && layout_width > 0.0 {
            layout_width
        } else {
            text_w
        };
        let align = self.style.text_align.unwrap_or(CssTextAlign::Left);
        let line_x = |advance: f32| match align {
            CssTextAlign::Center => (align_width - advance) / 2.0,
            CssTextAlign::Right | CssTextAlign::End => align_width - advance,
            _ => 0.0,
        };
        let block_x = line_x(text_w);

        let angle = if self.animate_angle {
            self.angle + time as f32 * self.speed * 360.0
        } else {
            self.angle
        };

        let angle_rad = angle * std::f32::consts::PI / 180.0;
        let (dir_x, dir_y) = (angle_rad.sin(), -angle_rad.cos());
        let cx = block_x + text_w / 2.0;
        let cy = text_h / 2.0;
        let half_len = ((text_w * angle_rad.sin()).abs() + (text_h * angle_rad.cos()).abs()) / 2.0;
        let start = Point::new(cx - dir_x * half_len, cy - dir_y * half_len);
        let end = Point::new(cx + dir_x * half_len, cy + dir_y * half_len);

        let (skia_colors, positions_vec): (Vec<skia_safe::Color>, Option<Vec<f32>>) =
            if let Some(explicit_stops) = self.stops.as_ref().filter(|s| !s.is_empty()) {
                let colors = explicit_stops
                    .iter()
                    .map(|stop| {
                        let (r, g, b, a) = parse_hex_color(&stop.color);
                        skia_safe::Color::from_argb(a, r, g, b)
                    })
                    .collect();
                let positions = explicit_stops.iter().map(|stop| stop.position).collect();
                (colors, Some(positions))
            } else {
                let colors = self
                    .colors
                    .iter()
                    .map(|hex| {
                        let (r, g, b, a) = parse_hex_color(hex);
                        skia_safe::Color::from_argb(a, r, g, b)
                    })
                    .collect();
                (colors, None)
            };

        let colors4f: Vec<Color4f> = skia_colors.iter().map(|c| Color4f::from(*c)).collect();
        let gradient_stops = Colors::new(
            &colors4f,
            positions_vec.as_deref(),
            skia_safe::TileMode::Clamp,
            None,
        );
        let grad = Gradient::new(gradient_stops, gradient::Interpolation::default());
        let shader = gradient::shaders::linear_gradient((start, end), &grad, None);

        let fill_paint = match shader {
            Some(shader) => {
                let mut p = skia_safe::Paint::default();
                p.set_anti_alias(true);
                p.set_shader(shader);
                p
            }
            None => {
                let fallback_hex = self
                    .stops
                    .as_ref()
                    .and_then(|s| s.first())
                    .map(|stop| stop.color.as_str())
                    .or_else(|| self.colors.first().map(String::as_str))
                    .unwrap_or("#FFFFFF");
                let mut p = paint_from_hex(fallback_hex);
                p.set_anti_alias(true);
                p
            }
        };

        if let Some(ref resolved) = props.char_animation {
            let schema_align = match align {
                CssTextAlign::Center => TextAlign::Center,
                CssTextAlign::Right | CssTextAlign::End => TextAlign::Right,
                _ => TextAlign::Left,
            };
            crate::intrinsic::render_char_animation(
                canvas,
                &font,
                &emoji_font,
                &fill_paint,
                letter_spacing,
                schema_align,
                align_width,
                line_height_val,
                ascent,
                &lines,
                resolved,
                time,
            );
            return;
        }

        for (i, line) in lines.iter().enumerate() {
            if line.is_empty() {
                continue;
            }
            let y = i as f32 * line_height_val + ascent;
            let x = line_x(measure_text_with_fallback(
                line,
                &font,
                &emoji_font,
                letter_spacing,
            ));
            draw_text_with_fallback(
                canvas,
                line,
                &font,
                &emoji_font,
                letter_spacing,
                x,
                y,
                &fill_paint,
            );
        }
    }
}

impl Painter for GradientText {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let (_, _, _, content_height) = layout.content_box();
        let content_height =
            (content_height > 0.0 && content_height.is_finite()).then_some(content_height);
        self.paint(canvas, layout.width, content_height, ctx.time, props, ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::css::style::CssStyle;
    use rustmotion_core::css::Length;
    use rustmotion_core::schema::{
        AnimationEffect, CharAnimationTiming, EasingType, TextAnimGranularity,
    };

    fn make_gradient_text(content: &str, white_space: Option<CssWhiteSpace>) -> GradientText {
        GradientText {
            content: content.into(),
            colors: default_colors(),
            stops: None,
            angle: default_angle(),
            animate_angle: false,
            speed: default_speed(),
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(28.0)),
                white_space,
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
        }
    }

    fn alpha_grid(surface: &mut skia_safe::Surface, width: i32, height: i32) -> Vec<u8> {
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (width, height),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (width * height * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (width * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        (0..(width * height) as usize)
            .map(|i| buf[i * 4 + 3])
            .collect()
    }

    fn test_ctx() -> PaintCtx {
        PaintCtx {
            time: 0.0,
            scenario_time: 0.0,
            scene_duration: 1.0,
            frame_index: 0,
            fps: 30,
            video_width: 1920,
            video_height: 1080,
            stagger_offset: 0.0,
        }
    }

    fn has_ink_in(grid: &[u8], surface_width: i32, x0: i32, x1: i32, y0: i32, y1: i32) -> bool {
        for y in y0..y1 {
            for x in x0..x1 {
                if grid[(y * surface_width + x) as usize] > 0 {
                    return true;
                }
            }
        }
        false
    }

    fn ink_x_span(grid: &[u8], surface_width: i32, height: i32) -> Option<(i32, i32)> {
        let mut span: Option<(i32, i32)> = None;
        for y in 0..height {
            for x in 0..surface_width {
                if grid[(y * surface_width + x) as usize] > 0 {
                    span = Some(match span {
                        None => (x, x),
                        Some((lo, hi)) => (lo.min(x), hi.max(x)),
                    });
                }
            }
        }
        span
    }

    fn ink_y_span(grid: &[u8], surface_width: i32, height: i32) -> Option<(i32, i32)> {
        let mut span: Option<(i32, i32)> = None;
        for y in 0..height {
            for x in 0..surface_width {
                if grid[(y * surface_width + x) as usize] > 0 {
                    span = Some(match span {
                        None => (y, y),
                        Some((lo, hi)) => (lo.min(y), hi.max(y)),
                    });
                }
            }
        }
        span
    }

    fn render_unpremul(gt: &GradientText, w: i32, h: i32) -> (Vec<u8>, Vec<u8>) {
        render_unpremul_at(gt, w, h, 0.0, &AnimatedProperties::default())
    }

    fn render_unpremul_at(
        gt: &GradientText,
        w: i32,
        h: i32,
        time: f64,
        props: &AnimatedProperties,
    ) -> (Vec<u8>, Vec<u8>) {
        let mut surface = skia_safe::surfaces::raster_n32_premul((w, h)).expect("raster surface");
        gt.paint(surface.canvas(), w as f32, None, time, props, &test_ctx());
        let snapshot = surface.image_snapshot();
        let info = skia_safe::ImageInfo::new(
            (w, h),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let mut buf = vec![0u8; (w * h * 4) as usize];
        assert!(snapshot.read_pixels(
            &info,
            &mut buf,
            (w * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        ));
        let alpha: Vec<u8> = (0..(w * h) as usize).map(|i| buf[i * 4 + 3]).collect();
        (buf, alpha)
    }

    fn best_pixel_in_column(buf: &[u8], alpha: &[u8], w: i32, h: i32, x: i32) -> (u8, u8, u8) {
        let (mut best_y, mut best_a) = (0i32, 0u8);
        for y in 0..h {
            let a = alpha[(y * w + x) as usize];
            if a > best_a {
                best_a = a;
                best_y = y;
            }
        }
        let i = ((best_y * w + x) * 4) as usize;
        (buf[i], buf[i + 1], buf[i + 2])
    }

    fn best_pixel_in_row(
        buf: &[u8],
        alpha: &[u8],
        w: i32,
        y: i32,
        x0: i32,
        x1: i32,
    ) -> (u8, u8, u8) {
        let (mut best_x, mut best_a) = (x0, 0u8);
        for x in x0..x1 {
            let a = alpha[(y * w + x) as usize];
            if a > best_a {
                best_a = a;
                best_x = x;
            }
        }
        let i = ((y * w + best_x) * 4) as usize;
        (buf[i], buf[i + 1], buf[i + 2])
    }

    fn nearest_inked_column(alpha: &[u8], w: i32, h: i32, target: i32) -> i32 {
        for radius in 0..w {
            for cand in [target - radius, target + radius] {
                if cand < 0 || cand >= w {
                    continue;
                }
                if (0..h).any(|y| alpha[(y * w + cand) as usize] > 0) {
                    return cand;
                }
            }
        }
        target
    }

    #[test]
    fn text_align_center_centres_the_line_in_the_box() {
        const W: i32 = 700;
        const H: i32 = 80;
        const BOX_W: f32 = 600.0;

        let mut gt = make_gradient_text("GRADIENT", Some(CssWhiteSpace::Nowrap));
        gt.style.text_align = Some(CssTextAlign::Center);

        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        gt.paint(
            surface.canvas(),
            BOX_W,
            None,
            0.0,
            &AnimatedProperties::default(),
            &test_ctx(),
        );
        let grid = alpha_grid(&mut surface, W, H);
        let (lo, hi) = ink_x_span(&grid, W, H).expect("centred gradient_text must paint something");

        let ink_centre = (lo + hi) as f32 / 2.0;
        let box_centre = BOX_W / 2.0;
        assert!(
            (ink_centre - box_centre).abs() <= 4.0,
            "centred gradient_text should sit on the box centre {box_centre}, \
             painted [{lo}, {hi}] with centre {ink_centre}"
        );
        assert!(
            lo > 40,
            "centred gradient_text must leave a left margin, first ink at {lo}"
        );
    }

    #[test]
    fn text_align_right_ends_the_line_on_the_box_edge() {
        const W: i32 = 700;
        const H: i32 = 80;
        const BOX_W: f32 = 600.0;

        let mut gt = make_gradient_text("GRADIENT", Some(CssWhiteSpace::Nowrap));
        gt.style.text_align = Some(CssTextAlign::Right);

        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        gt.paint(
            surface.canvas(),
            BOX_W,
            None,
            0.0,
            &AnimatedProperties::default(),
            &test_ctx(),
        );
        let grid = alpha_grid(&mut surface, W, H);
        let (lo, hi) = ink_x_span(&grid, W, H).expect("right-aligned gradient_text must paint");

        assert!(
            (hi as f32 - BOX_W).abs() <= 6.0,
            "right-aligned gradient_text should end on the box edge {BOX_W}, \
             painted [{lo}, {hi}]"
        );
    }

    #[test]
    fn no_text_align_still_starts_at_the_box_left_edge() {
        const W: i32 = 700;
        const H: i32 = 80;

        let gt = make_gradient_text("GRADIENT", Some(CssWhiteSpace::Nowrap));
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        gt.paint(
            surface.canvas(),
            600.0,
            None,
            0.0,
            &AnimatedProperties::default(),
            &test_ctx(),
        );
        let grid = alpha_grid(&mut surface, W, H);
        let (lo, _) = ink_x_span(&grid, W, H).expect("gradient_text must paint");

        assert!(
            lo < 8,
            "with no text-align the first ink should hug the left edge, got {lo}"
        );
    }

    #[test]
    fn centring_keeps_the_gradient_over_the_glyphs() {
        const W: i32 = 700;
        const H: i32 = 80;
        const BOX_W: f32 = 600.0;

        fn last_glyph_rgb(align: Option<CssTextAlign>) -> (u8, u8, u8) {
            let mut gt = make_gradient_text("GRADIENT", Some(CssWhiteSpace::Nowrap));
            gt.style.text_align = align;
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
            gt.paint(
                surface.canvas(),
                BOX_W,
                None,
                0.0,
                &AnimatedProperties::default(),
                &test_ctx(),
            );

            let snapshot = surface.image_snapshot();
            let info = skia_safe::ImageInfo::new(
                (W, H),
                skia_safe::ColorType::RGBA8888,
                skia_safe::AlphaType::Premul,
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
            let alpha: Vec<u8> = (0..(W * H) as usize).map(|i| buf[i * 4 + 3]).collect();
            let (_, hi) = ink_x_span(&alpha, W, H).expect("must paint");
            let (mut best_y, mut best_a) = (0i32, 0u8);
            for y in 0..H {
                let a = alpha[(y * W + hi) as usize];
                if a > best_a {
                    best_a = a;
                    best_y = y;
                }
            }
            let i = ((best_y * W + hi) * 4) as usize;
            (buf[i], buf[i + 1], buf[i + 2])
        }

        let left = last_glyph_rgb(None);
        let centre = last_glyph_rgb(Some(CssTextAlign::Center));
        let d = |a: u8, b: u8| (a as i32 - b as i32).abs();
        assert!(
            d(left.0, centre.0) <= 12 && d(left.1, centre.1) <= 12 && d(left.2, centre.2) <= 12,
            "the gradient must follow the aligned block: last glyph is {left:?} \
             left-aligned but {centre:?} centred"
        );
    }

    #[test]
    fn nowrap_paints_a_single_line_past_the_layout_width() {
        let gt = make_gradient_text(
            "the quick brown fox jumps over the lazy dog",
            Some(CssWhiteSpace::Nowrap),
        );
        const W: i32 = 600;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let canvas = surface.canvas();
        gt.paint(
            canvas,
            80.0,
            None,
            0.0,
            &AnimatedProperties::default(),
            &test_ctx(),
        );
        let grid = alpha_grid(&mut surface, W, H);

        assert!(
            has_ink_in(&grid, W, 300, W, 0, 45),
            "nowrap gradient_text must paint past its 80px box on line 1"
        );
        assert!(
            !has_ink_in(&grid, W, 0, W, 55, H),
            "nowrap gradient_text must stay on a single line"
        );
    }

    #[test]
    fn normal_white_space_wraps_within_the_layout_width() {
        let gt = make_gradient_text("the quick brown fox jumps over the lazy dog", None);
        const W: i32 = 600;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let canvas = surface.canvas();
        gt.paint(
            canvas,
            80.0,
            None,
            0.0,
            &AnimatedProperties::default(),
            &test_ctx(),
        );
        let grid = alpha_grid(&mut surface, W, H);

        assert!(
            !has_ink_in(&grid, W, 300, W, 0, 45),
            "wrapped gradient_text must not reach x∈[300,600) on line 1 within an 80px box"
        );
        assert!(
            has_ink_in(&grid, W, 0, W, 55, H),
            "wrapped gradient_text must spill onto a second line within the box width"
        );
    }

    #[test]
    fn rem_font_size_paints_visible_ink() {
        let gt = GradientText {
            content: "HELLO".into(),
            colors: default_colors(),
            stops: None,
            angle: default_angle(),
            animate_angle: false,
            speed: default_speed(),
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::String("2rem".into())),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
        };
        const W: i32 = 400;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let canvas = surface.canvas();
        gt.paint(
            canvas,
            300.0,
            None,
            0.0,
            &AnimatedProperties::default(),
            &test_ctx(),
        );
        let grid = alpha_grid(&mut surface, W, H);

        assert!(
            has_ink_in(&grid, W, 0, W, 0, 60),
            "gradient_text at font-size: 2rem must paint visible ink"
        );
    }

    fn autofit_gradient_text(
        content: &str,
        font_size: f32,
        white_space: Option<CssWhiteSpace>,
    ) -> GradientText {
        let mut gt = make_gradient_text(content, white_space);
        gt.style.font_size = Some(Length::Px(font_size));
        gt.style.text_autofit = Some(true);
        gt
    }

    fn max_ink_x(grid: &[u8], surface_width: i32, height: i32) -> Option<i32> {
        let mut max_x: Option<i32> = None;
        for y in 0..height {
            for x in (0..surface_width).rev() {
                if grid[(y * surface_width + x) as usize] > 0 {
                    max_x = Some(max_x.map_or(x, |m| m.max(x)));
                    break;
                }
            }
        }
        max_x
    }

    #[test]
    fn autofit_shrinks_a_nowrap_line_to_fit_and_paint_agrees_with_measure() {
        use crate::intrinsic::GradientTextIntrinsic;
        use rustmotion_core::engine::box_tree::{AvailableSpace, IntrinsicMeasure};

        let gt = autofit_gradient_text(
            "the quick brown fox jumps over the lazy dog",
            90.0,
            Some(CssWhiteSpace::Nowrap),
        );
        const BOX_W: f32 = 300.0;

        let (measured_w, _) = GradientTextIntrinsic::from_gradient_text(&gt).measure(
            (None, None),
            (AvailableSpace::Definite(BOX_W), AvailableSpace::MaxContent),
        );
        assert!(
            measured_w <= BOX_W + 0.5,
            "GradientTextIntrinsic itself must report a fit once autofit is on, got {measured_w}"
        );

        const W: i32 = 900;
        const H: i32 = 300;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let canvas = surface.canvas();
        gt.paint(
            canvas,
            BOX_W,
            None,
            0.0,
            &AnimatedProperties::default(),
            &test_ctx(),
        );
        let grid = alpha_grid(&mut surface, W, H);
        let ink_right = max_ink_x(&grid, W, H).expect("gradient_text must paint some ink");

        assert!(
            (ink_right as f32) <= measured_w + 3.0,
            "painted ink (right edge {ink_right}) must not exceed the box the intrinsic reserved \
             ({measured_w})"
        );
        assert!(
            (ink_right as f32) >= measured_w - 15.0,
            "painted ink (right edge {ink_right}) should land close to the measured width \
             ({measured_w}) — a big gap means measure and paint disagree on the resolved size"
        );
    }

    #[test]
    fn without_text_autofit_nowrap_still_bleeds_past_the_box_exactly_as_before() {
        let gt = make_gradient_text(
            "the quick brown fox jumps over the lazy dog",
            Some(CssWhiteSpace::Nowrap),
        );
        const W: i32 = 600;
        const H: i32 = 200;
        let mut surface = skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
        let canvas = surface.canvas();
        gt.paint(
            canvas,
            80.0,
            Some(45.0),
            0.0,
            &AnimatedProperties::default(),
            &test_ctx(),
        );
        let grid = alpha_grid(&mut surface, W, H);

        assert!(
            has_ink_in(&grid, W, 300, W, 0, 45),
            "without text-autofit, nowrap gradient_text must still bleed past its box"
        );
    }

    #[test]
    fn autofit_is_stable_across_frames_for_fixed_content() {
        let gt = autofit_gradient_text(
            "the quick brown fox jumps over the lazy dog",
            90.0,
            Some(CssWhiteSpace::Nowrap),
        );
        const W: i32 = 900;
        const H: i32 = 300;

        let render_at = |t: f64| -> Vec<u8> {
            let mut surface =
                skia_safe::surfaces::raster_n32_premul((W, H)).expect("raster surface");
            let canvas = surface.canvas();
            gt.paint(
                canvas,
                300.0,
                Some(60.0),
                t,
                &AnimatedProperties::default(),
                &test_ctx(),
            );
            alpha_grid(&mut surface, W, H)
        };

        let frame_a = render_at(0.0);
        let frame_b = render_at(0.9);
        assert_eq!(
            frame_a, frame_b,
            "fixed content in a fixed box must render byte-identically regardless of ctx.time"
        );
    }

    fn two_color_gradient(angle: f32) -> GradientText {
        GradientText {
            content: "IIIIIIIIIIIIIIII".into(),
            colors: vec!["#FF0000".into(), "#0000FF".into()],
            stops: None,
            angle,
            animate_angle: false,
            speed: default_speed(),
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(120.0)),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
        }
    }

    #[test]
    fn default_angle_ramps_left_to_right_across_the_glyphs() {
        let gt = two_color_gradient(default_angle());

        const W: i32 = 1200;
        const H: i32 = 300;
        let (buf, alpha) = render_unpremul(&gt, W, H);
        let (lo, hi) = ink_x_span(&alpha, W, H).expect("must paint some ink");

        let left = best_pixel_in_column(&buf, &alpha, W, H, (lo + 2).min(hi));
        let right = best_pixel_in_column(&buf, &alpha, W, H, (hi - 2).max(lo));

        assert!(
            left.0 as i32 - left.2 as i32 > 60,
            "with the default angle (CSS 90deg = to right), the left glyph edge should read \
             close to the first color (red), got {left:?}"
        );
        assert!(
            right.2 as i32 - right.0 as i32 > 60,
            "with the default angle (CSS 90deg = to right), the right glyph edge should read \
             close to the last color (blue), got {right:?}"
        );
    }

    #[test]
    fn gradient_line_length_is_the_box_projection_not_the_diagonal() {
        let gt = two_color_gradient(0.0);

        const W: i32 = 1200;
        const H: i32 = 300;
        let (buf, alpha) = render_unpremul(&gt, W, H);
        let (top, bottom) = ink_y_span(&alpha, W, H).expect("must paint some ink");
        let (lo_x, hi_x) = ink_x_span(&alpha, W, H).expect("must paint some ink");

        let top_row = (top + 3).min(bottom);
        let bottom_row = (bottom - 3).max(top);
        let top_px = best_pixel_in_row(&buf, &alpha, W, top_row, lo_x, hi_x + 1);
        let bottom_px = best_pixel_in_row(&buf, &alpha, W, bottom_row, lo_x, hi_x + 1);

        assert!(
            bottom_px.0 as i32 - bottom_px.2 as i32 > 60,
            "angle: 0 (CSS 'to top') must put the first color at the bottom edge of the glyph \
             box, got {bottom_px:?}"
        );
        assert!(
            top_px.2 as i32 - top_px.0 as i32 > 60,
            "angle: 0 (CSS 'to top') must reach the last color at the top edge — a gradient \
             line as long as the box diagonal would leave the glyphs sampling only the \
             unsaturated middle of the ramp, got {top_px:?}"
        );
    }

    #[test]
    fn explicit_stops_override_even_color_spacing() {
        fn three_color_gradient(stops: Option<Vec<GradientTextStop>>) -> GradientText {
            GradientText {
                content: "IIIIIIIIIIIIIIII".into(),
                colors: vec!["#FF0000".into(), "#00FF00".into(), "#0000FF".into()],
                stops,
                angle: 90.0,
                animate_angle: false,
                speed: default_speed(),
                timing: Default::default(),
                style: CssStyle {
                    font_size: Some(Length::Px(120.0)),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
            }
        }

        const W: i32 = 1200;
        const H: i32 = 300;

        let even = three_color_gradient(None);
        let (buf_even, alpha_even) = render_unpremul(&even, W, H);
        let (lo, hi) = ink_x_span(&alpha_even, W, H).expect("must paint some ink");
        let mid_x = nearest_inked_column(&alpha_even, W, H, (lo + hi) / 2);
        let mid_even = best_pixel_in_column(&buf_even, &alpha_even, W, H, mid_x);

        let skewed = three_color_gradient(Some(vec![
            GradientTextStop {
                color: "#FF0000".into(),
                position: 0.0,
            },
            GradientTextStop {
                color: "#00FF00".into(),
                position: 0.9,
            },
            GradientTextStop {
                color: "#0000FF".into(),
                position: 1.0,
            },
        ]));
        let (buf_skewed, alpha_skewed) = render_unpremul(&skewed, W, H);
        let mid_skewed = best_pixel_in_column(&buf_skewed, &alpha_skewed, W, H, mid_x);

        assert!(
            mid_even.1 as i32 > mid_skewed.1 as i32 + 40,
            "moving the middle stop from its even-spacing position (0.5) to 0.9 must move the \
             ramp: the text's midpoint should read far less green than the even-spacing case \
             (even={mid_even:?}, skewed={mid_skewed:?})"
        );
    }

    fn props_for(gt: &GradientText) -> AnimatedProperties {
        AnimatedProperties {
            char_animation: rustmotion_core::engine::animator::extract_effects(&gt.style.animation)
                .char_animation,
            ..Default::default()
        }
    }

    fn soft_pixel_fraction(
        grid: &[u8],
        surface_width: i32,
        x0: i32,
        x1: i32,
        y0: i32,
        y1: i32,
    ) -> f32 {
        let mut inked = 0u32;
        let mut soft = 0u32;
        for y in y0..y1 {
            for x in x0..x1 {
                let a = grid[(y * surface_width + x) as usize];
                if a > 0 {
                    inked += 1;
                    if a < 250 {
                        soft += 1;
                    }
                }
            }
        }
        if inked == 0 {
            return 0.0;
        }
        soft as f32 / inked as f32
    }

    #[test]
    fn char_blur_in_animates_a_gradient_text_word_instead_of_painting_it_sharp_immediately() {
        let mut gt = make_gradient_text("BLUR", Some(CssWhiteSpace::Nowrap));
        gt.style.font_size = Some(Length::Px(100.0));
        gt.style.animation = vec![AnimationEffect::CharBlurIn(CharAnimationTiming {
            delay: 0.0,
            duration: 0.5,
            stagger: 0.03,
            granularity: TextAnimGranularity::Word,
            easing: EasingType::Linear,
            ..Default::default()
        })];

        const W: i32 = 700;
        const H: i32 = 220;
        let props = props_for(&gt);

        let early = alpha_grid(
            &mut {
                let mut s = skia_safe::surfaces::raster_n32_premul((W, H)).expect("surface");
                gt.paint(s.canvas(), W as f32, None, 0.15, &props, &test_ctx());
                s
            },
            W,
            H,
        );
        let settled = alpha_grid(
            &mut {
                let mut s = skia_safe::surfaces::raster_n32_premul((W, H)).expect("surface");
                gt.paint(s.canvas(), W as f32, None, 1.0, &props, &test_ctx());
                s
            },
            W,
            H,
        );

        assert!(
            has_ink_in(&early, W, 0, W, 0, H),
            "the word must have started painting by t=0.15 — a char_* preset on gradient_text \
             used to be silently ignored and paint the word fully sharp from frame 0"
        );

        let early_soft = soft_pixel_fraction(&early, W, 0, W, 0, H);
        let settled_soft = soft_pixel_fraction(&settled, W, 0, W, 0, H);
        assert!(
            early_soft > settled_soft + 0.15,
            "mid-reveal soft-pixel fraction ({early_soft:.3}) must be clearly higher than the \
             settled fraction ({settled_soft:.3}) — gradient_text must actually blur while \
             animating, not just render the sharp glyph unconditionally"
        );
        assert!(
            settled_soft < 0.25,
            "settled frame should read as sharp text, not blur (soft fraction {settled_soft:.3})"
        );
    }

    #[test]
    fn char_animation_never_restarts_the_gradient_ramp_per_unit() {
        let mut gt = two_color_gradient(default_angle());
        gt.content = "IIII IIII IIII IIII".into();
        gt.style.white_space = Some(CssWhiteSpace::Nowrap);
        gt.style.animation = vec![AnimationEffect::CharFadeIn(CharAnimationTiming {
            delay: 0.0,
            duration: 0.1,
            stagger: 0.0,
            granularity: TextAnimGranularity::Word,
            easing: EasingType::Linear,
            ..Default::default()
        })];

        const W: i32 = 1400;
        const H: i32 = 300;
        let props = props_for(&gt);
        let (buf, alpha) = render_unpremul_at(&gt, W, H, 5.0, &props);
        let (lo, hi) = ink_x_span(&alpha, W, H).expect("must paint some ink");

        let left = best_pixel_in_column(&buf, &alpha, W, H, (lo + 2).min(hi));
        let right = best_pixel_in_column(&buf, &alpha, W, H, (hi - 2).max(lo));

        assert!(
            left.0 as i32 - left.2 as i32 > 60,
            "the leftmost word must still read close to the ramp's first colour (red) once \
             settled — got {left:?}"
        );
        assert!(
            right.2 as i32 - right.0 as i32 > 60,
            "the rightmost word must still read close to the ramp's last colour (blue) — \
             splitting the run into per-word animated units must not restart the gradient for \
             each word, got {right:?}"
        );
    }
}
