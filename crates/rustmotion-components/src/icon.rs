use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ColorType, ImageInfo, Paint, Rect, SamplingOptions};

use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{asset_cache, fetch_icon_svg, icon_cache_key};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Icon {
    /// Iconify identifier: "prefix:name" (e.g. "lucide:home", "mdi:account")
    pub icon: String,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
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
        let target_w = (layout.width as u32).max(1);
        let target_h = (layout.height as u32).max(1);
        // Oversampling (crisp edges under sub-pixel positioning / scale
        // animation) and the cache key are computed together by
        // `icon_cache_key` — see its doc for why that matters (issue #166):
        // this used to be a local `const OVERSAMPLE` here plus an
        // independent `format!` in `preload.rs`'s prefetcher, and the two
        // could never agree.
        let (render_w, render_h, cache_key) = icon_cache_key(&self.icon, color, target_w, target_h);

        let cache = asset_cache();
        let img = if let Some(cached) = cache.get(&cache_key) {
            cached.clone()
        } else {
            let Ok(svg_data) = fetch_icon_svg(&self.icon, color, render_w, render_h) else {
                // Preload (issue #167 item 2) already hard-fails when an
                // icon cannot be resolved via disk cache or network, so this
                // branch is defense in depth (paint_content can run without
                // a preceding prefetch, or the layout-derived target size
                // here can differ from the preloader's style-based
                // estimate, producing a genuine cache miss). Guarded so a
                // single offline/typo'd icon does not spam once per frame
                // over a render that can be 1000+ frames long.
                if crate::warn_once_for(&format!("icon-fetch-failed:{}", self.icon)) {
                    eprintln!(
                        "Warning: icon '{}' could not be loaded (checked the disk cache and \
                         the network) — nothing will be painted for it.",
                        self.icon
                    );
                }
                return;
            };

            let opt = usvg::Options::default();
            let Ok(tree) = usvg::Tree::from_data(&svg_data, &opt) else {
                return;
            };

            let svg_size = tree.size();
            let Some(mut pixmap) = tiny_skia::Pixmap::new(render_w, render_h) else {
                return;
            };

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
            else {
                return;
            };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn base_ctx() -> PaintCtx {
        PaintCtx {
            time: 0.0,
            scenario_time: 0.0,
            scene_duration: 1.0,
            frame_index: 0,
            fps: 30,
            video_width: 100,
            video_height: 100,
            stagger_offset: 0.0,
        }
    }

    fn solid_image() -> skia_safe::Image {
        let px = [255u8, 0, 255, 255];
        let mut data = Vec::with_capacity(4 * 4);
        for _ in 0..4 {
            data.extend_from_slice(&px);
        }
        let img_info = ImageInfo::new(
            (2, 2),
            ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let skia_data = skia_safe::Data::new_copy(&data);
        skia_safe::images::raster_from_data(&img_info, skia_data, 2 * 4).expect("sentinel image")
    }

    /// Regression for issue #166: proves the painter's cache-key formula is
    /// literally `icon_cache_key`. Pre-populate `asset_cache()` under the
    /// exact key `preload.rs`'s prefetcher now computes for a 40×40 target,
    /// then confirm the painter finds and paints it — instead of falling
    /// through to `fetch_icon_svg` for a nonsense icon id (which pre-fix, on
    /// a key mismatch, is exactly what would have happened every time).
    #[test]
    fn painter_finds_the_entry_preload_would_have_written() {
        let icon = Icon {
            icon: "test-suite:icon-cache-key-agreement".to_string(),
            timing: Default::default(),
            style: CssStyle::default(),
            timeline: Vec::new(),
            stagger: None,
        };
        let target_w = 40u32;
        let target_h = 40u32;
        let color = icon.style.color_str_or("#FFFFFF");
        let (_, _, key) = icon_cache_key(&icon.icon, color, target_w, target_h);

        asset_cache().insert(key.clone(), solid_image());

        let layout = BoxLayout {
            width: target_w as f32,
            height: target_h as f32,
            ..Default::default()
        };
        let ctx = base_ctx();
        let props = AnimatedProperties::default();
        let mut surface =
            skia_safe::surfaces::raster_n32_premul((target_w as i32, target_h as i32)).unwrap();
        {
            let canvas = surface.canvas();
            icon.paint_content(canvas, &layout, &props, &ctx);
        }

        // Cleanup so this entry does not leak into other tests sharing the
        // process-global asset_cache.
        asset_cache().remove(&key);

        let snapshot = surface.image_snapshot();
        let info = ImageInfo::new(
            (target_w as i32, target_h as i32),
            ColorType::RGBA8888,
            skia_safe::AlphaType::Premul,
            None,
        );
        let mut buf = vec![0u8; (target_w * target_h * 4) as usize];
        let ok = snapshot.read_pixels(
            &info,
            &mut buf,
            (target_w * 4) as usize,
            skia_safe::IPoint::new(0, 0),
            skia_safe::image::CachingHint::Disallow,
        );
        assert!(ok, "pixel read should succeed");
        let has_ink = buf.chunks(4).any(|px| px[3] > 0);
        assert!(
            has_ink,
            "painter must have found and painted the cache entry preload.rs would have \
             written under the same key — if the keys disagree, nothing paints"
        );
    }
}
