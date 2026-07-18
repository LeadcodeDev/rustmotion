use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ColorType, ImageInfo, Paint, Rect};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::asset_cache;
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Svg {
    #[serde(default)]
    pub src: Option<String>,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

rustmotion_core::impl_traits!(Svg {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Painter for Svg {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        _ctx: &PaintCtx,
    ) {
        let target_w_opt: Option<u32> = if layout.width > 0.0 {
            Some(layout.width as u32)
        } else {
            None
        };
        let target_h_opt: Option<u32> = if layout.height > 0.0 {
            Some(layout.height as u32)
        } else {
            None
        };

        let cache_key = if let Some(ref src) = self.src {
            format!(
                "svg:{}:{}x{}",
                src,
                target_w_opt.unwrap_or(0),
                target_h_opt.unwrap_or(0)
            )
        } else if let Some(ref data) = self.data {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            data.hash(&mut hasher);
            format!(
                "svg-inline:{}:{}x{}",
                hasher.finish(),
                target_w_opt.unwrap_or(0),
                target_h_opt.unwrap_or(0)
            )
        } else {
            return;
        };

        let cache = asset_cache();
        let img = if let Some(cached) = cache.get(&cache_key) {
            cached.clone()
        } else {
            let svg_data = if let Some(ref src) = self.src {
                let Ok(data) = std::fs::read(src) else { return };
                data
            } else if let Some(ref data) = self.data {
                data.as_bytes().to_vec()
            } else {
                return;
            };

            let opt = usvg::Options::default();
            let Ok(tree) = usvg::Tree::from_data(&svg_data, &opt) else {
                return;
            };

            let svg_size = tree.size();
            let target_w = target_w_opt.unwrap_or(svg_size.width() as u32);
            let target_h = target_h_opt.unwrap_or(svg_size.height() as u32);

            let Some(mut pixmap) = tiny_skia::Pixmap::new(target_w, target_h) else {
                return;
            };

            let scale_x = target_w as f32 / svg_size.width();
            let scale_y = target_h as f32 / svg_size.height();
            let transform = tiny_skia::Transform::from_scale(scale_x, scale_y);

            resvg::render(&tree, transform, &mut pixmap.as_mut());

            let img_data = skia_safe::Data::new_copy(pixmap.data());
            let img_info = ImageInfo::new(
                (target_w as i32, target_h as i32),
                ColorType::RGBA8888,
                skia_safe::AlphaType::Premul,
                None,
            );
            let Some(decoded) =
                skia_safe::images::raster_from_data(&img_info, img_data, target_w as usize * 4)
            else {
                return;
            };
            cache.insert(cache_key, decoded.clone());
            decoded
        };

        let dst = Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
        let paint = Paint::default();
        canvas.draw_image_rect(img, None, dst, &paint);
    }
}
