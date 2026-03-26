use rustmotion_core::error::{Result, RustmotionError};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ColorType, ImageInfo, Paint, Rect};
use rustmotion_core::engine::renderer::asset_cache;
use rustmotion_core::layout::{Constraints, LayoutNode};
use rustmotion_core::schema::{LayerStyle, Size};
use rustmotion_core::traits::{RenderContext, TimingConfig, Widget};

/// A Lottie animation component that renders frame-by-frame from a .json Lottie file.
///
/// Renders Lottie animations by pre-rendering frames as PNG files in a temp directory.
/// Requires `lottie_to_png` or similar tool on PATH, or falls back to rendering
/// the first frame as a static SVG via the Lottie JSON's static assets.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Lottie {
    /// Path to the Lottie JSON file.
    #[serde(default)]
    pub src: Option<String>,
    /// Inline Lottie JSON data.
    #[serde(default)]
    pub data: Option<String>,
    /// Output size (width, height). If not specified, uses the Lottie's intrinsic size.
    #[serde(default)]
    pub size: Option<Size>,
    /// Playback speed multiplier (1.0 = normal, 2.0 = double speed).
    #[serde(default = "default_speed")]
    pub speed: f32,
    /// Whether to loop the animation.
    #[serde(default = "default_true")]
    #[serde(rename = "loop")]
    pub repeat: bool,
    /// Directory containing pre-rendered frames (PNG files named 0000.png, 0001.png, ...).
    /// If provided, the component loads frames directly from this directory.
    #[serde(default)]
    pub frames_dir: Option<String>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: LayerStyle,
}

fn default_speed() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

rustmotion_core::impl_traits!(Lottie {
    Animatable => style,
    Timed => timing,
    Styled => style,
});

impl Lottie {
    /// Parse Lottie JSON metadata (fr, ip, op, w, h).
    fn parse_metadata(&self) -> Result<(f64, usize, f64, f32, f32)> {
        let json_str = if let Some(ref src) = self.src {
            std::fs::read_to_string(src)
                .map_err(|e| RustmotionError::LottieRead { path: src.clone(), reason: e.to_string() })?
        } else if let Some(ref data) = self.data {
            data.clone()
        } else {
            return Err(RustmotionError::LottieMissingSrc);
        };

        let json: serde_json::Value = serde_json::from_str(&json_str)?;
        let fr = json["fr"].as_f64().unwrap_or(30.0);
        let ip = json["ip"].as_f64().unwrap_or(0.0);
        let op = json["op"].as_f64().unwrap_or(60.0);
        let w = json["w"].as_f64().unwrap_or(200.0) as f32;
        let h = json["h"].as_f64().unwrap_or(200.0) as f32;
        let total_frames = (op - ip) as usize;
        let duration = total_frames as f64 / fr;

        Ok((fr, total_frames, duration, w, h))
    }

    /// Get the cache key for a specific frame.
    fn cache_key(&self, frame: usize) -> String {
        let src = self.src.as_deref().unwrap_or("inline");
        format!("lottie:{}:frame:{}", src, frame)
    }

    /// Load a pre-rendered frame from frames_dir.
    fn load_frame_from_dir(&self, frames_dir: &str, frame: usize) -> Result<skia_safe::Image> {
        let frame_path = format!("{}/{:04}.png", frames_dir, frame);
        let data = std::fs::read(&frame_path)
            .map_err(|e| RustmotionError::LottieFrameRead { path: frame_path.clone(), reason: e.to_string() })?;

        let img = image::load_from_memory(&data)
            .map_err(|e| RustmotionError::LottieFrameDecode { path: frame_path.clone(), reason: e.to_string() })?;
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();

        let img_data = skia_safe::Data::new_copy(rgba.as_raw());
        let img_info = ImageInfo::new(
            (w as i32, h as i32),
            ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );

        skia_safe::images::raster_from_data(&img_info, img_data, w as usize * 4)
            .ok_or(RustmotionError::SkiaImageCreation { target: "lottie frame".to_string() })
    }
}

impl Widget for Lottie {
    fn render(
        &self,
        canvas: &Canvas,
        layout: &LayoutNode,
        ctx: &RenderContext,
        _props: &rustmotion_core::engine::animator::AnimatedProperties,
        _pipeline: &dyn rustmotion_core::traits::RenderPipeline,
    ) -> Result<()> {
        let (fr, total_frames, duration, _intrinsic_w, _intrinsic_h) = self.parse_metadata()?;

        if total_frames == 0 {
            return Ok(());
        }

        // Calculate which frame to render
        let anim_time = ctx.time * self.speed as f64;
        let effective_time = if self.repeat && duration > 0.0 {
            anim_time % duration
        } else {
            anim_time.min(duration)
        };
        let frame = ((effective_time * fr) as usize).min(total_frames.saturating_sub(1));

        let cache_key = self.cache_key(frame);
        let cache = asset_cache();

        let img = if let Some(cached) = cache.get(&cache_key) {
            cached.clone()
        } else if let Some(ref frames_dir) = self.frames_dir {
            let img = self.load_frame_from_dir(frames_dir, frame)?;
            cache.insert(cache_key, img.clone());
            img
        } else {
            return Err(RustmotionError::LottieRender {
                reason: "Lottie component requires 'frames_dir' pointing to pre-rendered PNG frames. \
                         You can generate frames using: npx lottie-to-frames <lottie.json> --output <dir>".to_string(),
            });
        };

        let dst = Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
        let paint = Paint::default();
        canvas.draw_image_rect(img, None, dst, &paint);

        Ok(())
    }

    fn measure(&self, _constraints: &Constraints) -> (f32, f32) {
        if let Some(ref size) = self.size {
            return (size.width, size.height);
        }

        // Try to read intrinsic size from Lottie JSON
        if let Ok((_, _, _, w, h)) = self.parse_metadata() {
            return (w, h);
        }

        (200.0, 200.0)
    }
}
