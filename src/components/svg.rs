use anyhow::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ColorType, ImageInfo, Paint, Rect};

use crate::engine::renderer::asset_cache;
use crate::layout::{Constraints, LayoutNode};
use crate::schema::{LayerStyle, Size};
use crate::traits::{AnimationConfig, RenderContext, TimingConfig, Widget};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Svg {
    #[serde(default)]
    pub src: Option<String>,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(flatten)]
    pub animation: AnimationConfig,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

crate::impl_traits!(Svg {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Widget for Svg {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, _ctx: &RenderContext, _props: &crate::engine::animator::AnimatedProperties) -> Result<()> {
        let (target_w_opt, target_h_opt) = match &self.size {
            Some(size) => (Some(size.width as u32), Some(size.height as u32)),
            None => (None, None),
        };

        let cache_key = if let Some(ref src) = self.src {
            format!("svg:{}:{}x{}", src, target_w_opt.unwrap_or(0), target_h_opt.unwrap_or(0))
        } else if let Some(ref data) = self.data {
            use std::collections::hash_map::DefaultHasher;
            use std::hash::{Hash, Hasher};
            let mut hasher = DefaultHasher::new();
            data.hash(&mut hasher);
            format!("svg-inline:{}:{}x{}", hasher.finish(), target_w_opt.unwrap_or(0), target_h_opt.unwrap_or(0))
        } else {
            return Err(anyhow::anyhow!("SVG layer must have either 'src' or 'data'"));
        };

        let cache = asset_cache();
        let img = if let Some(cached) = cache.get(&cache_key) {
            cached.clone()
        } else {
            let svg_data = if let Some(ref src) = self.src {
                std::fs::read(src)
                    .map_err(|e| anyhow::anyhow!("Failed to load SVG '{}': {}", src, e))?
            } else if let Some(ref data) = self.data {
                data.as_bytes().to_vec()
            } else {
                unreachable!()
            };

            let opt = usvg::Options::default();
            let tree = usvg::Tree::from_data(&svg_data, &opt)
                .map_err(|e| anyhow::anyhow!("Failed to parse SVG: {}", e))?;

            let svg_size = tree.size();
            let target_w = target_w_opt.unwrap_or(svg_size.width() as u32);
            let target_h = target_h_opt.unwrap_or(svg_size.height() as u32);

            let mut pixmap = tiny_skia::Pixmap::new(target_w, target_h)
                .ok_or_else(|| anyhow::anyhow!("Failed to create pixmap for SVG"))?;

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
            let decoded = skia_safe::images::raster_from_data(&img_info, img_data, target_w as usize * 4)
                .ok_or_else(|| anyhow::anyhow!("Failed to create Skia image from SVG"))?;
            cache.insert(cache_key, decoded.clone());
            decoded
        };

        let dst = Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
        let paint = Paint::default();
        canvas.draw_image_rect(img, None, dst, &paint);

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        match &self.size {
            Some(s) => (s.width, s.height),
            None => (100.0, 100.0),
        }
    }
}
