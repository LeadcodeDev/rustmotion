use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ColorType, ImageInfo, Paint, Rect, SamplingOptions};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{asset_cache, fetch_icon_svg};
use rustmotion_core::schema::{AnimationEffect, Size, TimelineStep};
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Icon {
    /// Iconify identifier: "prefix:name" (e.g. "lucide:home", "mdi:account")
    pub icon: String,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default, deserialize_with = "rustmotion_core::schema::deserialize_animation_effects")]
    pub animation: Vec<AnimationEffect>,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Icon {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Painter for Icon {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        let color = self.style.color_str_or("#FFFFFF");
        // Oversample 2× so the rasterized SVG stays crisp under sub-pixel
        // positioning and minor scale animations. Skia's high-quality
        // sampling downscales to layout size without softening edges.
        const OVERSAMPLE: u32 = 2;
        let target_w = (layout.width as u32).max(1);
        let target_h = (layout.height as u32).max(1);
        let render_w = target_w * OVERSAMPLE;
        let render_h = target_h * OVERSAMPLE;

        let cache_key = format!("icon:{}:{}:{}x{}", self.icon, color, render_w, render_h);

        let cache = asset_cache();
        let img = if let Some(cached) = cache.get(&cache_key) {
            cached.clone()
        } else {
            let Ok(svg_data) = fetch_icon_svg(&self.icon, color, render_w, render_h) else { return };

            let opt = usvg::Options::default();
            let Ok(tree) = usvg::Tree::from_data(&svg_data, &opt) else { return };

            let svg_size = tree.size();
            let Some(mut pixmap) = tiny_skia::Pixmap::new(render_w, render_h) else { return };

            let scale_x = render_w as f32 / svg_size.width();
            let scale_y = render_h as f32 / svg_size.height();
            let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);

            resvg::render(&tree, transform, &mut pixmap.as_mut());

            let img_data = skia_safe::Data::new_copy(pixmap.data());
            let img_info = ImageInfo::new(
                (render_w as i32, render_h as i32),
                ColorType::RGBA8888,
                skia_safe::AlphaType::Premul,
                None,
            );
            let Some(decoded) =
                skia_safe::images::raster_from_data(&img_info, img_data, render_w as usize * 4)
            else { return };
            cache.insert(cache_key, decoded.clone());
            decoded
        };

        let dst = Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
        let paint = Paint::default();
        canvas.draw_image_rect_with_sampling_options(
            img,
            None,
            dst,
            SamplingOptions::from(skia_safe::CubicResampler::mitchell()),
            &paint,
        );
    }
}
