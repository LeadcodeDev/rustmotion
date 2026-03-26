use rustmotion_core::error::Result;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ColorType, ImageInfo, Paint, Rect};

use rustmotion_core::engine::renderer::{asset_cache, fetch_icon_svg};
use rustmotion_core::error::RustmotionError;
use rustmotion_core::layout::{Constraints, LayoutNode};
use rustmotion_core::schema::{LayerStyle, Size};
use rustmotion_core::traits::{RenderContext, TimingConfig, Widget};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Icon {
    /// Iconify identifier: "prefix:name" (e.g. "lucide:home", "mdi:account")
    pub icon: String,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

rustmotion_core::impl_traits!(Icon {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Widget for Icon {
    fn render(&self, canvas: &Canvas, layout: &LayoutNode, _ctx: &RenderContext, _props: &rustmotion_core::engine::animator::AnimatedProperties, _pipeline: &dyn rustmotion_core::traits::RenderPipeline) -> Result<()> {
        let color = self.style.color_or("#FFFFFF");
        let target_w = layout.width as u32;
        let target_h = layout.height as u32;

        let cache_key = format!("icon:{}:{}:{}x{}", self.icon, color, target_w, target_h);

        let cache = asset_cache();
        let img = if let Some(cached) = cache.get(&cache_key) {
            cached.clone()
        } else {
            let svg_data = fetch_icon_svg(&self.icon, color, target_w, target_h)?;

            let opt = usvg::Options::default();
            let tree = usvg::Tree::from_data(&svg_data, &opt)
                .map_err(|e| RustmotionError::IconParse { icon: self.icon.clone(), reason: e.to_string() })?;

            let svg_size = tree.size();
            let render_w = target_w.max(1);
            let render_h = target_h.max(1);

            let mut pixmap = tiny_skia::Pixmap::new(render_w, render_h)
                .ok_or_else(|| RustmotionError::PixmapCreation { target: "icon".to_string() })?;

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
            let decoded = skia_safe::images::raster_from_data(&img_info, img_data, render_w as usize * 4)
                .ok_or_else(|| RustmotionError::SkiaImageCreation { target: "icon".to_string() })?;
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
            None => (24.0, 24.0),
        }
    }
}
