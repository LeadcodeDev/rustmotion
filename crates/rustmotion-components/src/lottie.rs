use rustmotion_core::css::CssStyle;
use rustmotion_core::engine::animator::AnimatedProperties;
use rustmotion_core::engine::layout_pass::BoxLayout;
use rustmotion_core::engine::renderer::{asset_cache, draw_photo};
use rustmotion_core::error::{Result, RustmotionError};
use rustmotion_core::schema::TimelineStep;
use rustmotion_core::traits::{PaintCtx, Painter, TimingConfig};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use skia_safe::{Canvas, ColorType, ImageInfo, Paint, Rect};

/// A Lottie animation component that renders frame-by-frame from a .json Lottie file.
///
/// With the `lottie-native` feature (default), Lottie JSON files are rendered natively
/// via the `thorvg` CPU rasterizer — no external tools required. Priority:
/// 1. `frames_dir` — backward-compatible pre-rendered PNG frames directory.
/// 2. Native thorvg path — resolves `data` (inline JSON) or `src` (file path).
///
/// Without `lottie-native`, only `frames_dir` works; all other inputs produce no output.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Lottie {
    /// Path to the Lottie JSON file.
    #[serde(default)]
    pub src: Option<String>,
    /// Inline Lottie JSON data.
    #[serde(default)]
    pub data: Option<String>,
    /// Playback speed multiplier (1.0 = normal, 2.0 = double speed).
    #[serde(default = "default_speed")]
    pub speed: f32,
    /// Whether to loop the animation.
    #[serde(default = "default_true")]
    #[serde(rename = "loop")]
    pub repeat: bool,
    /// Directory containing pre-rendered frames (PNG files named 0000.png, 0001.png, ...).
    /// If provided, the component loads frames directly from this directory.
    /// Takes priority over the native thorvg path.
    #[serde(default)]
    pub frames_dir: Option<String>,
    #[serde(flatten)]
    pub timing: TimingConfig,
    #[serde(default)]
    pub style: CssStyle,
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
    #[serde(default)]
    pub stagger: Option<f32>,
}

fn default_speed() -> f32 {
    1.0
}

fn default_true() -> bool {
    true
}

rustmotion_core::impl_traits!(Lottie {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});

impl Lottie {
    fn parse_metadata(&self) -> Result<(f64, usize, f64, f32, f32)> {
        let json_str = if let Some(ref src) = self.src {
            std::fs::read_to_string(src).map_err(|e| RustmotionError::LottieRead {
                path: src.clone(),
                reason: e.to_string(),
            })?
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

    fn cache_key(&self, frame: usize) -> String {
        let src = self.src.as_deref().unwrap_or("inline");
        format!("lottie:{}:frame:{}", src, frame)
    }

    fn load_frame_from_dir(&self, frames_dir: &str, frame: usize) -> Result<skia_safe::Image> {
        let frame_path = format!("{}/{:04}.png", frames_dir, frame);
        let data = std::fs::read(&frame_path).map_err(|e| RustmotionError::LottieFrameRead {
            path: frame_path.clone(),
            reason: e.to_string(),
        })?;

        let img =
            image::load_from_memory(&data).map_err(|e| RustmotionError::LottieFrameDecode {
                path: frame_path.clone(),
                reason: e.to_string(),
            })?;
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();

        let img_data = skia_safe::Data::new_copy(rgba.as_raw());
        let img_info = ImageInfo::new(
            (w as i32, h as i32),
            ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );

        skia_safe::images::raster_from_data(&img_info, img_data, w as usize * 4).ok_or(
            RustmotionError::SkiaImageCreation {
                target: "lottie frame".to_string(),
            },
        )
    }
}

#[cfg(feature = "lottie-native")]
mod native {
    use std::hash::Hash;
    use std::sync::{Arc, OnceLock};

    use dashmap::DashMap;
    use rustmotion_core::engine::renderer::draw_photo;

    const CACHE_MAX_ENTRIES: usize = 128;

    #[derive(Clone, PartialEq, Eq, Hash, Debug)]
    pub(super) struct FrameKey {
        pub src_hash: u64,
        pub frame_index: u32,
        pub width: u32,
        pub height: u32,
    }

    impl FrameKey {
        pub(super) fn new(json_bytes: &[u8], frame_index: u32, width: u32, height: u32) -> Self {
            let src_hash = fnv1a(json_bytes);
            Self {
                src_hash,
                frame_index,
                width,
                height,
            }
        }
    }

    pub(super) fn fnv1a(bytes: &[u8]) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for &b in bytes {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
        h
    }

    type NativeCacheMap = Arc<DashMap<FrameKey, Arc<Vec<u8>>>>;

    static NATIVE_LOTTIE_CACHE: OnceLock<NativeCacheMap> = OnceLock::new();

    pub(super) fn native_lottie_cache() -> &'static NativeCacheMap {
        NATIVE_LOTTIE_CACHE.get_or_init(|| Arc::new(DashMap::new()))
    }

    thread_local! {
        static THORVG_ENGINE: std::cell::RefCell<Option<thorvg::Thorvg>> =
            const { std::cell::RefCell::new(None) };
    }

    fn with_engine<F, R>(f: F) -> R
    where
        F: FnOnce(&thorvg::Thorvg) -> R,
    {
        THORVG_ENGINE.with(|cell| {
            let mut guard = cell.borrow_mut();
            if guard.is_none() {
                *guard = thorvg::Thorvg::init(0).ok();
            }
            f(guard.as_ref().expect("thorvg init failed"))
        })
    }

    static FAILED_SOURCES: OnceLock<DashMap<u64, ()>> = OnceLock::new();

    fn failed_sources() -> &'static DashMap<u64, ()> {
        FAILED_SOURCES.get_or_init(DashMap::new)
    }

    pub(super) fn render_frame(
        json_bytes: &[u8],
        frame_index: u32,
        width: u32,
        height: u32,
    ) -> Option<Arc<Vec<u8>>> {
        use thorvg::{ColorSpace, EngineOption};

        let key = FrameKey::new(json_bytes, frame_index, width, height);
        let cache = native_lottie_cache();
        if let Some(cached) = cache.get(&key) {
            return Some(cached.clone());
        }

        let src_hash = key.src_hash;

        if failed_sources().contains_key(&src_hash) {
            return None;
        }

        let rgba = with_engine(|engine| -> Option<Vec<u8>> {
            use thorvg::Paint as ThorPaint;

            let mut buffer = vec![0u32; (width * height) as usize];

            let mut canvas = engine.sw_canvas(EngineOption::Default).ok()?;
            unsafe {
                canvas
                    .set_target(&mut buffer, width, width, height, ColorSpace::ABGR8888)
                    .ok()?
            };

            let mut anim = engine.lottie_animation().ok()?;
            anim.load_data(json_bytes).ok()?;
            anim.set_size(width as f32, height as f32).ok()?;

            let total = anim.total_frame().ok()?;
            if total <= 0.0 {
                return None;
            }
            let clamped = (frame_index as f32).min(total - 1.0).max(0.0);
            let _ = anim.set_frame(clamped);

            let dup = anim.picture().duplicate()?;
            canvas.add(dup).ok()?;
            canvas.draw(true).ok()?;
            canvas.sync().ok()?;

            let byte_len = buffer.len() * 4;
            let mut out = Vec::with_capacity(byte_len);
            let byte_slice =
                unsafe { std::slice::from_raw_parts(buffer.as_ptr() as *const u8, byte_len) };
            out.extend_from_slice(byte_slice);
            Some(out)
        });

        match rgba {
            None => {
                eprintln!(
                    "[rustmotion] lottie-native: failed to render source (hash {:016x}); \
                     further errors for this source will be suppressed",
                    src_hash
                );
                failed_sources().insert(src_hash, ());
                None
            }
            Some(bytes) => {
                let arc = Arc::new(bytes);
                if cache.len() >= CACHE_MAX_ENTRIES {
                    cache.clear();
                }
                cache.insert(key, arc.clone());
                Some(arc)
            }
        }
    }

    pub(super) fn frame_at_time(
        t: f64,
        speed: f32,
        repeat: bool,
        fr: f64,
        total_frames: usize,
        duration: f64,
    ) -> u32 {
        if total_frames == 0 || duration <= 0.0 {
            return 0;
        }
        let anim_time = t * speed as f64;
        let effective = if repeat {
            anim_time % duration
        } else {
            anim_time.min(duration)
        };
        let f = (effective * fr) as usize;
        f.min(total_frames.saturating_sub(1)) as u32
    }

    pub(super) fn resolve_json(lottie: &super::Lottie) -> Option<Vec<u8>> {
        if let Some(ref data) = lottie.data {
            return Some(data.as_bytes().to_vec());
        }
        if let Some(ref src) = lottie.src {
            match std::fs::read(src) {
                Ok(bytes) => return Some(bytes),
                Err(e) => {
                    let path_hash = fnv1a(src.as_bytes());
                    if !failed_sources().contains_key(&path_hash) {
                        eprintln!(
                            "[rustmotion] lottie-native: cannot read '{}': {}; \
                             further errors for this source will be suppressed",
                            src, e
                        );
                        failed_sources().insert(path_hash, ());
                    }
                    return None;
                }
            }
        }
        None
    }

    pub(super) fn paint_native(
        lottie: &super::Lottie,
        canvas: &skia_safe::Canvas,
        layout: &rustmotion_core::engine::layout_pass::BoxLayout,
        ctx: &rustmotion_core::traits::PaintCtx,
    ) {
        let json_bytes = match resolve_json(lottie) {
            Some(b) => b,
            None => return,
        };

        let Ok((fr, total_frames, duration, _w, _h)) = lottie.parse_metadata() else {
            return;
        };

        let w = layout.width as u32;
        let h = layout.height as u32;
        if w == 0 || h == 0 {
            return;
        }

        let frame_index = frame_at_time(
            ctx.time,
            lottie.speed,
            lottie.repeat,
            fr,
            total_frames,
            duration,
        );

        let rgba = match render_frame(&json_bytes, frame_index, w, h) {
            Some(r) => r,
            None => return,
        };

        let img_data = skia_safe::Data::new_copy(&rgba);
        let img_info = skia_safe::ImageInfo::new(
            (w as i32, h as i32),
            skia_safe::ColorType::RGBA8888,
            skia_safe::AlphaType::Unpremul,
            None,
        );
        let Some(img) = skia_safe::images::raster_from_data(&img_info, img_data, w as usize * 4)
        else {
            return;
        };

        let dst = skia_safe::Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
        let paint = skia_safe::Paint::default();
        draw_photo(canvas, img, dst, &paint);
    }

    #[cfg(test)]
    pub(super) use frame_at_time as test_frame_at_time;
    #[cfg(test)]
    pub(super) use native_lottie_cache as test_cache;
    #[cfg(test)]
    pub(super) use render_frame as test_render_frame;
    #[cfg(test)]
    pub(super) use resolve_json as test_resolve_json;
}

impl Painter for Lottie {
    fn paint_content(
        &self,
        canvas: &Canvas,
        layout: &BoxLayout,
        _props: &AnimatedProperties,
        ctx: &PaintCtx,
    ) {
        let Ok((fr, total_frames, duration, _intrinsic_w, _intrinsic_h)) = self.parse_metadata()
        else {
            return;
        };

        if total_frames == 0 {
            return;
        }

        let anim_time = ctx.time * self.speed as f64;
        let effective_time = if self.repeat && duration > 0.0 {
            anim_time % duration
        } else {
            anim_time.min(duration)
        };
        let frame = ((effective_time * fr) as usize).min(total_frames.saturating_sub(1));

        if let Some(ref frames_dir) = self.frames_dir {
            let cache_key = self.cache_key(frame);
            let cache = asset_cache();

            let img = if let Some(cached) = cache.get(&cache_key) {
                cached.clone()
            } else {
                let Ok(img) = self.load_frame_from_dir(frames_dir, frame) else {
                    return;
                };
                cache.insert(cache_key, img.clone());
                img
            };

            let dst = Rect::from_xywh(0.0, 0.0, layout.width, layout.height);
            let paint = Paint::default();
            draw_photo(canvas, img, dst, &paint);
            return;
        }

        #[cfg(feature = "lottie-native")]
        {
            native::paint_native(self, canvas, layout, ctx);
        }
    }
}

#[cfg(all(test, feature = "lottie-native"))]
mod tests {
    use super::native::{test_cache, test_frame_at_time, test_render_frame};

    const RED_LOTTIE: &str = r#"{
        "v": "5.7.4", "fr": 30, "ip": 0, "op": 30, "w": 100, "h": 100,
        "layers": [{
            "ddd": 0, "ind": 1, "ty": 4, "nm": "rect", "sr": 1,
            "ks": {"o": {"a": 0, "k": 100}, "r": {"a": 0, "k": 0},
                    "p": {"a": 0, "k": [50, 50, 0]}, "a": {"a": 0, "k": [0, 0, 0]},
                    "s": {"a": 0, "k": [100, 100, 100]}},
            "shapes": [{"ty": "gr", "it": [
                {"ty": "rc", "p": {"a": 0, "k": [0, 0]}, "s": {"a": 0, "k": [80, 80]}, "r": {"a": 0, "k": 0}},
                {"ty": "fl", "c": {"a": 0, "k": [1, 0, 0, 1]}, "o": {"a": 0, "k": 100}},
                {"ty": "tr", "p": {"a": 0, "k": [0, 0]}, "a": {"a": 0, "k": [0, 0]},
                 "s": {"a": 0, "k": [100, 100]}, "r": {"a": 0, "k": 0}, "o": {"a": 0, "k": 100}}
            ]}],
            "ip": 0, "op": 30, "st": 0
        }]
    }"#;

    const W: u32 = 100;
    const H: u32 = 100;

    fn render_mid() -> Vec<u8> {
        test_render_frame(RED_LOTTIE.as_bytes(), 15, W, H)
            .expect("render_mid must succeed")
            .as_ref()
            .clone()
    }

    #[test]
    fn pixel_byte_order_red_lottie() {
        let buf = render_mid();
        assert_eq!(buf.len(), (W * H * 4) as usize);

        let mut red_sum: u64 = 0;
        let mut blue_sum: u64 = 0;
        for px in buf.as_chunks::<4>().0.iter() {
            let r = px[0] as u64;
            let _g = px[1] as u64;
            let b = px[2] as u64;
            let a = px[3] as u64;
            if a > 0 {
                red_sum += r;
                blue_sum += b;
            }
        }
        assert!(
            red_sum > 200_000,
            "expected dominant red, got red_sum={red_sum} blue_sum={blue_sum}"
        );
        assert!(
            blue_sum < 1000,
            "expected ~0 blue, got blue_sum={blue_sum} red_sum={red_sum}"
        );
    }

    #[test]
    fn repeat_true_wraps_to_same_frame() {
        let fr = 30.0f64;
        let total = 30usize;
        let dur = 1.0f64;

        let fi_half = test_frame_at_time(0.5, 1.0, true, fr, total, dur);
        let fi_wrap = test_frame_at_time(1.5, 1.0, true, fr, total, dur);
        assert_eq!(fi_half, fi_wrap, "repeat=true: t=1.5 should wrap to t=0.5");

        let buf_half =
            test_render_frame(RED_LOTTIE.as_bytes(), fi_half, W, H).expect("render half");
        let buf_wrap =
            test_render_frame(RED_LOTTIE.as_bytes(), fi_wrap, W, H).expect("render wrap");
        assert_eq!(*buf_half, *buf_wrap);
    }

    #[test]
    fn repeat_false_clamps_to_last_frame() {
        let fr = 30.0f64;
        let total = 30usize;
        let dur = 1.0f64;

        let fi_clamped = test_frame_at_time(1.5, 1.0, false, fr, total, dur);
        let fi_last = test_frame_at_time(1.0, 1.0, false, fr, total, dur);
        assert_eq!(fi_clamped, 29);
        assert_eq!(fi_last, 29);
    }

    #[test]
    fn speed_multiplier_equivalent_frame() {
        let fr = 30.0f64;
        let total = 30usize;
        let dur = 1.0f64;

        let fi_fast = test_frame_at_time(0.25, 2.0, false, fr, total, dur);
        let fi_normal = test_frame_at_time(0.5, 1.0, false, fr, total, dur);
        assert_eq!(
            fi_fast, fi_normal,
            "speed=2.0 at t=0.25 should equal speed=1.0 at t=0.5"
        );

        let buf_fast =
            test_render_frame(RED_LOTTIE.as_bytes(), fi_fast, W, H).expect("render fast");
        let buf_normal =
            test_render_frame(RED_LOTTIE.as_bytes(), fi_normal, W, H).expect("render normal");
        assert_eq!(*buf_fast, *buf_normal);
    }

    #[test]
    fn invalid_json_no_panic_zero_pixels() {
        let result = test_render_frame(b"not valid json at all!!!", 0, W, H);
        if let Some(buf) = result {
            let nonzero = buf.iter().any(|&b| b != 0);
            assert!(
                !nonzero,
                "invalid JSON should produce zero-pixel output, got non-zero pixels"
            );
        }
    }

    #[test]
    fn frames_dir_priority_no_native_cache_entry() {
        use super::{native, Lottie};

        test_cache().clear();

        let lottie = Lottie {
            src: None,
            data: Some(RED_LOTTIE.to_string()),
            speed: 1.0,
            repeat: false,
            frames_dir: Some("/non/existent/frames_dir".to_string()),
            timing: Default::default(),
            style: Default::default(),
            timeline: vec![],
            stagger: None,
        };

        use super::native::FrameKey;
        let expected_key = FrameKey::new(RED_LOTTIE.as_bytes(), 0, 100, 100);

        let _ = native::test_resolve_json(&lottie);

        assert!(
            !test_cache().contains_key(&expected_key),
            "native cache must not be populated when frames_dir takes priority"
        );
    }
}
