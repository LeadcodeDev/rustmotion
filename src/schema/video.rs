use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::animation::{Animation, AnimationPreset, EasingType, PresetConfig};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Scenario {
    #[serde(default = "default_version")]
    pub version: String,
    pub video: VideoConfig,
    #[serde(default)]
    pub audio: Vec<AudioTrack>,
    #[serde(default)]
    pub fonts: Vec<FontEntry>,
    #[serde(default)]
    pub scenes: Vec<SceneEntry>,
}

/// A scenario with all includes expanded — safe to pass to the rendering pipeline.
#[derive(Debug)]
pub struct ResolvedScenario {
    pub video: VideoConfig,
    pub audio: Vec<AudioTrack>,
    pub fonts: Vec<FontEntry>,
    pub scenes: Vec<Scene>,
}

/// An entry in the `scenes` array: either a concrete scene or an include directive.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum SceneEntry {
    /// A regular scene defined inline.
    Scene(Scene),
    /// A reference to an external scenario file whose scenes will be injected here.
    Include(IncludeDirective),
}

/// Directive to inject scenes from an external scenario file.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct IncludeDirective {
    /// Path (relative to parent file) or URL (http/https) to a scenario JSON file.
    pub include: String,
    /// Only include scenes at these 0-based indices. When absent, all scenes are included.
    #[serde(default)]
    pub scenes: Option<Vec<usize>>,
}

/// Font file to load at startup
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FontEntry {
    pub path: String,
    pub family: String,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct AudioTrack {
    pub src: String,
    #[serde(default)]
    pub start: f64,
    #[serde(default)]
    pub end: Option<f64>,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub fade_in: Option<f64>,
    #[serde(default)]
    pub fade_out: Option<f64>,
    #[serde(default)]
    pub volume_keyframes: Vec<VolumeKeyframe>,
}

/// Dynamic volume control point
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VolumeKeyframe {
    pub time: f64,
    pub volume: f32,
    #[serde(default)]
    pub easing: EasingType,
}

fn default_volume() -> f32 {
    1.0
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct VideoConfig {
    pub width: u32,
    pub height: u32,
    #[serde(default = "default_fps")]
    pub fps: u32,
    #[serde(default = "default_background")]
    pub background: String,
    #[serde(default)]
    pub codec: Option<VideoCodec>,
    #[serde(default)]
    pub crf: Option<u8>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Scene {
    pub duration: f64,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub children: Vec<crate::components::ChildComponent>,
    #[serde(default)]
    pub transition: Option<Transition>,
    #[serde(default)]
    pub freeze_at: Option<f64>,
    /// Flex layout for automatic layer positioning
    #[serde(default)]
    pub layout: Option<SceneLayout>,
    /// Animated background gradient
    #[serde(default, rename = "animated-background")]
    pub animated_background: Option<AnimatedBackground>,
    /// Virtual camera with animatable x, y, zoom, rotation.
    #[serde(default)]
    pub camera: Option<Camera>,
}

/// Virtual camera for pan/zoom/rotation effects at the scene level.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Camera {
    /// Camera center X offset from scene center (pixels). Default: 0.
    #[serde(default)]
    pub x: f32,
    /// Camera center Y offset from scene center (pixels). Default: 0.
    #[serde(default)]
    pub y: f32,
    /// Zoom factor. 1.0 = no zoom, 2.0 = 2x zoom in, 0.5 = zoom out.
    #[serde(default = "default_camera_zoom")]
    pub zoom: f32,
    /// Rotation in degrees around the scene center. Default: 0.
    #[serde(default)]
    pub rotation: f32,
    /// Keyframe animations for camera properties.
    #[serde(default)]
    pub keyframes: Vec<CameraKeyframe>,
}

/// A keyframe for a camera property.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CameraKeyframe {
    /// The camera property to animate: "x", "y", "zoom", "rotation".
    pub property: String,
    /// Time-value pairs for the animation.
    pub values: Vec<CameraKeyframePoint>,
    /// Easing function for interpolation.
    #[serde(default)]
    pub easing: EasingType,
}

/// A single time-value point in a camera keyframe.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CameraKeyframePoint {
    /// Time in seconds (relative to scene start).
    pub time: f64,
    /// Value at this time.
    pub value: f32,
}

fn default_camera_zoom() -> f32 {
    1.0
}

/// Animated background configuration for scenes.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AnimatedBackground {
    /// Gradient colors (hex strings).
    #[serde(default)]
    pub colors: Vec<String>,
    /// Animation speed (degrees/sec for gradient, pixels/sec for others; default 30).
    #[serde(default = "default_bg_speed")]
    pub speed: f32,
    /// Gradient type: "linear" or "radial" (default "linear").
    #[serde(default = "default_bg_type")]
    pub gradient_type: GradientType,
    /// Preset pattern: "gradient_shift" (default), "concentric_circles", "grid_dots".
    #[serde(default)]
    pub preset: Option<String>,
    /// Dot/circle size for grid_dots and concentric_circles presets.
    #[serde(default = "default_bg_element_size")]
    pub element_size: f32,
    /// Spacing between elements for grid_dots preset.
    #[serde(default = "default_bg_spacing")]
    pub spacing: f32,
}

fn default_bg_element_size() -> f32 {
    4.0
}

fn default_bg_spacing() -> f32 {
    60.0
}

fn default_bg_speed() -> f32 {
    30.0
}

fn default_bg_type() -> GradientType {
    GradientType::Linear
}

/// Scene-level flex layout configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SceneLayout {
    #[serde(default)]
    pub direction: Option<CardDirection>,
    #[serde(default)]
    pub gap: Option<f32>,
    #[serde(default)]
    pub align_items: Option<CardAlign>,
    #[serde(default)]
    pub justify_content: Option<CardJustify>,
    #[serde(default)]
    pub padding: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Transition {
    #[serde(rename = "type")]
    pub transition_type: TransitionType,
    #[serde(default = "default_transition_duration")]
    pub duration: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransitionType {
    Fade,
    WipeLeft,
    WipeRight,
    WipeUp,
    WipeDown,
    ZoomIn,
    ZoomOut,
    Flip,
    ClockWipe,
    Iris,
    Slide,
    Dissolve,
    None,
}

fn default_transition_duration() -> f64 {
    0.5
}

// --- Card types ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CardDirection {
    Column,
    Row,
    ColumnReverse,
    RowReverse,
}

impl Default for CardDirection {
    fn default() -> Self {
        Self::Column
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CardAlign {
    Start,
    Center,
    End,
    Stretch,
}

impl Default for CardAlign {
    fn default() -> Self {
        Self::Start
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CardJustify {
    Start,
    Center,
    End,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
}

impl Default for CardJustify {
    fn default() -> Self {
        Self::Start
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CardBorder {
    pub color: String,
    #[serde(default = "default_card_border_width")]
    pub width: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GradientBorder {
    pub colors: Vec<String>,
    #[serde(default = "default_gradient_border_width")]
    pub width: f32,
    #[serde(default)]
    pub angle: f32,
}

fn default_gradient_border_width() -> f32 {
    2.0
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CardShadow {
    pub color: String,
    #[serde(default)]
    pub offset_x: f32,
    #[serde(default)]
    pub offset_y: f32,
    #[serde(default)]
    pub blur: f32,
}

/// Inner shadow configuration (inset shadow).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct InnerShadow {
    pub color: String,
    #[serde(default)]
    pub offset_x: f32,
    #[serde(default)]
    pub offset_y: f32,
    #[serde(default = "default_inner_shadow_blur")]
    pub blur: f32,
}

fn default_inner_shadow_blur() -> f32 {
    10.0
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Spacing {
    Uniform(f32),
    Sides {
        top: f32,
        right: f32,
        bottom: f32,
        left: f32,
    },
}

impl Spacing {
    pub fn resolve(&self) -> (f32, f32, f32, f32) {
        match self {
            Spacing::Uniform(v) => (*v, *v, *v, *v),
            Spacing::Sides {
                top,
                right,
                bottom,
                left,
            } => (*top, *right, *bottom, *left),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CardDisplay {
    Flex,
    Grid,
}

impl Default for CardDisplay {
    fn default() -> Self {
        Self::Flex
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GridTrack {
    Px(f32),
    Fr(f32),
    Auto,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GridPlacement {
    #[serde(default)]
    pub start: Option<i32>,
    #[serde(default)]
    pub span: Option<u32>,
}

// --- Animation effects (nested inside LayerStyle as typed array) ---

/// A single animation effect. Discriminated by `"type"` in JSON.
/// Each preset name is a valid type, plus special types: glow, wiggle, keyframes, motion_blur.
///
/// Examples:
/// ```json
/// { "name": "fade_in_up", "delay": 0.3, "duration": 0.6 }
/// { "name": "glow", "color": "#F68F2B", "radius": 16, "intensity": 1.2 }
/// { "name": "wiggle", "property": "translate_y", "amplitude": 5, "frequency": 2 }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "name", rename_all = "snake_case")]
pub enum AnimationEffect {
    // --- Entrance presets ---
    FadeIn(AnimationTiming),
    FadeInUp(AnimationTiming),
    FadeInDown(AnimationTiming),
    FadeInLeft(AnimationTiming),
    FadeInRight(AnimationTiming),
    SlideInLeft(AnimationTiming),
    SlideInRight(AnimationTiming),
    SlideInUp(AnimationTiming),
    SlideInDown(AnimationTiming),
    ScaleIn(AnimationTiming),
    BounceIn(AnimationTiming),
    BlurIn(AnimationTiming),
    RotateIn(AnimationTiming),
    ElasticIn(AnimationTiming),
    // --- Exit presets ---
    FadeOut(AnimationTiming),
    FadeOutUp(AnimationTiming),
    FadeOutDown(AnimationTiming),
    SlideOutLeft(AnimationTiming),
    SlideOutRight(AnimationTiming),
    SlideOutUp(AnimationTiming),
    SlideOutDown(AnimationTiming),
    ScaleOut(AnimationTiming),
    BounceOut(AnimationTiming),
    BlurOut(AnimationTiming),
    RotateOut(AnimationTiming),
    // --- Continuous presets ---
    Pulse(AnimationTiming),
    Float(AnimationTiming),
    Shake(AnimationTiming),
    Spin(AnimationTiming),
    // --- 3D presets ---
    FlipInX(AnimationTiming),
    FlipInY(AnimationTiming),
    FlipOutX(AnimationTiming),
    FlipOutY(AnimationTiming),
    TiltIn(AnimationTiming),
    // --- Special presets ---
    Typewriter(AnimationTiming),
    WipeLeft(AnimationTiming),
    WipeRight(AnimationTiming),
    // --- Non-preset effects ---
    Glow(GlowConfig),
    Wiggle(WiggleConfig),
    Keyframes(KeyframesConfig),
    MotionBlur(MotionBlurConfig),
}

impl AnimationEffect {
    /// If this is a preset variant, return the corresponding AnimationPreset and timing.
    pub fn as_preset(&self) -> Option<(AnimationPreset, &AnimationTiming)> {
        match self {
            Self::FadeIn(t) => Some((AnimationPreset::FadeIn, t)),
            Self::FadeInUp(t) => Some((AnimationPreset::FadeInUp, t)),
            Self::FadeInDown(t) => Some((AnimationPreset::FadeInDown, t)),
            Self::FadeInLeft(t) => Some((AnimationPreset::FadeInLeft, t)),
            Self::FadeInRight(t) => Some((AnimationPreset::FadeInRight, t)),
            Self::SlideInLeft(t) => Some((AnimationPreset::SlideInLeft, t)),
            Self::SlideInRight(t) => Some((AnimationPreset::SlideInRight, t)),
            Self::SlideInUp(t) => Some((AnimationPreset::SlideInUp, t)),
            Self::SlideInDown(t) => Some((AnimationPreset::SlideInDown, t)),
            Self::ScaleIn(t) => Some((AnimationPreset::ScaleIn, t)),
            Self::BounceIn(t) => Some((AnimationPreset::BounceIn, t)),
            Self::BlurIn(t) => Some((AnimationPreset::BlurIn, t)),
            Self::RotateIn(t) => Some((AnimationPreset::RotateIn, t)),
            Self::ElasticIn(t) => Some((AnimationPreset::ElasticIn, t)),
            Self::FadeOut(t) => Some((AnimationPreset::FadeOut, t)),
            Self::FadeOutUp(t) => Some((AnimationPreset::FadeOutUp, t)),
            Self::FadeOutDown(t) => Some((AnimationPreset::FadeOutDown, t)),
            Self::SlideOutLeft(t) => Some((AnimationPreset::SlideOutLeft, t)),
            Self::SlideOutRight(t) => Some((AnimationPreset::SlideOutRight, t)),
            Self::SlideOutUp(t) => Some((AnimationPreset::SlideOutUp, t)),
            Self::SlideOutDown(t) => Some((AnimationPreset::SlideOutDown, t)),
            Self::ScaleOut(t) => Some((AnimationPreset::ScaleOut, t)),
            Self::BounceOut(t) => Some((AnimationPreset::BounceOut, t)),
            Self::BlurOut(t) => Some((AnimationPreset::BlurOut, t)),
            Self::RotateOut(t) => Some((AnimationPreset::RotateOut, t)),
            Self::Pulse(t) => Some((AnimationPreset::Pulse, t)),
            Self::Float(t) => Some((AnimationPreset::Float, t)),
            Self::Shake(t) => Some((AnimationPreset::Shake, t)),
            Self::Spin(t) => Some((AnimationPreset::Spin, t)),
            Self::FlipInX(t) => Some((AnimationPreset::FlipInX, t)),
            Self::FlipInY(t) => Some((AnimationPreset::FlipInY, t)),
            Self::FlipOutX(t) => Some((AnimationPreset::FlipOutX, t)),
            Self::FlipOutY(t) => Some((AnimationPreset::FlipOutY, t)),
            Self::TiltIn(t) => Some((AnimationPreset::TiltIn, t)),
            Self::Typewriter(t) => Some((AnimationPreset::Typewriter, t)),
            Self::WipeLeft(t) => Some((AnimationPreset::WipeLeft, t)),
            Self::WipeRight(t) => Some((AnimationPreset::WipeRight, t)),
            _ => None,
        }
    }
}

/// Timing configuration for preset animations.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct AnimationTiming {
    /// Delay before animation starts (seconds).
    #[serde(default)]
    pub delay: f64,
    /// Animation duration (seconds).
    #[serde(default = "default_animation_duration")]
    pub duration: f64,
    /// Loop the animation continuously.
    #[serde(default, rename = "loop")]
    pub repeat: bool,
}

fn default_animation_duration() -> f64 {
    0.8
}

impl Default for AnimationTiming {
    fn default() -> Self {
        Self {
            delay: 0.0,
            duration: 0.8,
            repeat: false,
        }
    }
}

/// Per-character text animation configuration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CharAnimation {
    /// Animation preset: "scale_in", "fade_in", "wave", "bounce", "rotate_in".
    #[serde(default = "default_char_preset")]
    pub preset: CharAnimPreset,
    /// Delay between each character in seconds.
    #[serde(default = "default_char_stagger")]
    pub stagger: f32,
    /// Duration of each character's animation in seconds.
    #[serde(default = "default_char_duration")]
    pub duration: f32,
    /// Easing function.
    #[serde(default)]
    pub easing: EasingType,
    /// Initial delay before the first character starts.
    #[serde(default)]
    pub delay: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "snake_case")]
pub enum CharAnimPreset {
    /// Each character scales from 0 to 1.
    #[default]
    ScaleIn,
    /// Each character fades from 0 to 1 opacity.
    FadeIn,
    /// Characters oscillate vertically in a wave pattern.
    Wave,
    /// Each character bounces in (scale overshoot).
    Bounce,
    /// Each character rotates in from a random angle.
    RotateIn,
    /// Each character slides up from below.
    SlideUp,
}

fn default_char_preset() -> CharAnimPreset {
    CharAnimPreset::ScaleIn
}

fn default_char_stagger() -> f32 {
    0.03
}

fn default_char_duration() -> f32 {
    0.4
}

impl AnimationTiming {
    /// Convert to PresetConfig for compatibility with resolve_animations.
    pub fn to_preset_config(&self) -> PresetConfig {
        PresetConfig {
            delay: self.delay,
            duration: self.duration,
            repeat: self.repeat,
        }
    }
}

/// Custom keyframe animations configuration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct KeyframesConfig {
    pub keyframes: Vec<Animation>,
    #[serde(default)]
    pub delay: f64,
    #[serde(default = "default_animation_duration")]
    pub duration: f64,
    #[serde(default, rename = "loop")]
    pub repeat: bool,
}

/// Motion blur configuration.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct MotionBlurConfig {
    #[serde(default)]
    pub intensity: f32,
}

/// Deserialize `animation` as either a single AnimationEffect or a Vec.
fn deserialize_animation_effects<'de, D>(deserializer: D) -> Result<Vec<AnimationEffect>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct OneOrMany;

    impl<'de> de::Visitor<'de> for OneOrMany {
        type Value = Vec<AnimationEffect>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a single animation effect or an array of animation effects")
        }

        fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            Vec::deserialize(de::value::SeqAccessDeserializer::new(seq))
        }

        fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            let effect =
                AnimationEffect::deserialize(de::value::MapAccessDeserializer::new(map))?;
            Ok(vec![effect])
        }
    }

    deserializer.deserialize_any(OneOrMany)
}

// --- Unified LayerStyle ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct LayerStyle {
    // Common visual
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub padding: Option<Spacing>,
    #[serde(default)]
    pub margin: Option<Spacing>,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default, rename = "border-radius")]
    pub border_radius: Option<f32>,
    #[serde(default)]
    pub border: Option<CardBorder>,
    #[serde(default, rename = "box-shadow")]
    pub box_shadow: Option<CardShadow>,
    #[serde(default, rename = "text-shadow")]
    pub text_shadow: Option<TextShadow>,
    // Typography
    #[serde(default, rename = "font-size")]
    pub font_size: Option<f32>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default, rename = "font-family")]
    pub font_family: Option<String>,
    #[serde(default, rename = "font-weight")]
    pub font_weight: Option<FontWeight>,
    #[serde(default, rename = "font-style")]
    pub font_style: Option<FontStyleType>,
    #[serde(default, rename = "text-align")]
    pub text_align: Option<TextAlign>,
    #[serde(default, rename = "letter-spacing")]
    pub letter_spacing: Option<f32>,
    #[serde(default, rename = "line-height")]
    pub line_height: Option<f32>,
    // SVG/Shape
    #[serde(default)]
    pub stroke: Option<Stroke>,
    #[serde(default)]
    pub fill: Option<Fill>,
    // Flex container
    #[serde(default, rename = "flex-direction")]
    pub flex_direction: Option<CardDirection>,
    #[serde(default)]
    pub gap: Option<f32>,
    #[serde(default, rename = "align-items")]
    pub align_items: Option<CardAlign>,
    #[serde(default, rename = "justify-content")]
    pub justify_content: Option<CardJustify>,
    #[serde(default, rename = "flex-wrap")]
    pub flex_wrap: Option<bool>,
    #[serde(default)]
    pub display: Option<CardDisplay>,
    // Grid container
    #[serde(default, rename = "grid-template-columns")]
    pub grid_template_columns: Option<Vec<GridTrack>>,
    #[serde(default, rename = "grid-template-rows")]
    pub grid_template_rows: Option<Vec<GridTrack>>,
    // Per-child flex
    #[serde(default, rename = "flex-grow")]
    pub flex_grow: Option<f32>,
    #[serde(default, rename = "flex-shrink")]
    pub flex_shrink: Option<f32>,
    #[serde(default, rename = "flex-basis")]
    pub flex_basis: Option<f32>,
    #[serde(default, rename = "align-self")]
    pub align_self: Option<CardAlign>,
    // Per-child grid
    #[serde(default, rename = "grid-column")]
    pub grid_column: Option<GridPlacement>,
    #[serde(default, rename = "grid-row")]
    pub grid_row: Option<GridPlacement>,
    // Text highlight background
    #[serde(default, rename = "text-background")]
    pub text_background: Option<TextBackground>,
    // Glassmorphism / advanced effects
    #[serde(default, rename = "backdrop-blur")]
    pub backdrop_blur: Option<f32>,
    #[serde(default, rename = "gradient-border")]
    pub gradient_border: Option<GradientBorder>,
    // Visual effects
    #[serde(default)]
    pub filter: Option<FilterConfig>,
    #[serde(default, rename = "drop-shadow")]
    pub drop_shadow: Option<DropShadow>,
    #[serde(default, rename = "blend-mode")]
    pub blend_mode: Option<BlendMode>,
    #[serde(default, rename = "clip-path")]
    pub clip_path: Option<String>,
    #[serde(default, rename = "aspect-ratio")]
    pub aspect_ratio: Option<f32>,
    #[serde(default, rename = "text-gradient")]
    pub text_gradient: Option<TextGradient>,
    // Inner shadow (inset shadow)
    #[serde(default, rename = "inner-shadow")]
    pub inner_shadow: Option<InnerShadow>,
    // Motion path: SVG path string that elements follow
    #[serde(default, rename = "motion-path")]
    pub motion_path: Option<String>,
    // Stagger: automatic delay offset per child in a container (seconds)
    #[serde(default)]
    pub stagger: Option<f32>,
    // Animation effects (accepts single object or array)
    #[serde(default, deserialize_with = "deserialize_animation_effects")]
    pub animation: Vec<AnimationEffect>,
}

impl Default for LayerStyle {
    fn default() -> Self {
        Self {
            opacity: 1.0,
            padding: None,
            margin: None,
            background: None,
            border_radius: None,
            border: None,
            box_shadow: None,
            text_shadow: None,
            font_size: None,
            color: None,
            font_family: None,
            font_weight: None,
            font_style: None,
            text_align: None,
            letter_spacing: None,
            line_height: None,
            stroke: None,
            fill: None,
            flex_direction: None,
            gap: None,
            align_items: None,
            justify_content: None,
            flex_wrap: None,
            display: None,
            grid_template_columns: None,
            grid_template_rows: None,
            flex_grow: None,
            flex_shrink: None,
            flex_basis: None,
            align_self: None,
            grid_column: None,
            grid_row: None,
            text_background: None,
            backdrop_blur: None,
            gradient_border: None,
            filter: None,
            drop_shadow: None,
            blend_mode: None,
            clip_path: None,
            aspect_ratio: None,
            text_gradient: None,
            inner_shadow: None,
            motion_path: None,
            stagger: None,
            animation: Vec::new(),
        }
    }
}

impl LayerStyle {
    pub fn font_size_or(&self, default: f32) -> f32 {
        self.font_size.unwrap_or(default)
    }
    pub fn color_or<'a>(&'a self, default: &'a str) -> &'a str {
        self.color.as_deref().unwrap_or(default)
    }
    pub fn font_family_or<'a>(&'a self, default: &'a str) -> &'a str {
        self.font_family.as_deref().unwrap_or(default)
    }
    pub fn font_weight_or(&self, default: FontWeight) -> FontWeight {
        self.font_weight.clone().unwrap_or(default)
    }
    pub fn font_style_or(&self, default: FontStyleType) -> FontStyleType {
        self.font_style.clone().unwrap_or(default)
    }
    pub fn text_align_or(&self, default: TextAlign) -> TextAlign {
        self.text_align.clone().unwrap_or(default)
    }
    pub fn border_radius_or(&self, default: f32) -> f32 {
        self.border_radius.unwrap_or(default)
    }
    pub fn gap_or(&self, default: f32) -> f32 {
        self.gap.unwrap_or(default)
    }
    pub fn flex_direction_or(&self, default: CardDirection) -> CardDirection {
        self.flex_direction.clone().unwrap_or(default)
    }
    pub fn align_items_or(&self, default: CardAlign) -> CardAlign {
        self.align_items.clone().unwrap_or(default)
    }
    pub fn justify_content_or(&self, default: CardJustify) -> CardJustify {
        self.justify_content.clone().unwrap_or(default)
    }
    pub fn flex_wrap_or(&self, default: bool) -> bool {
        self.flex_wrap.unwrap_or(default)
    }
    pub fn display_or(&self, default: CardDisplay) -> CardDisplay {
        self.display.clone().unwrap_or(default)
    }
    pub fn padding_resolved(&self) -> (f32, f32, f32, f32) {
        self.padding
            .as_ref()
            .map(|p| p.resolve())
            .unwrap_or((0.0, 0.0, 0.0, 0.0))
    }
    pub fn margin_resolved(&self) -> (f32, f32, f32, f32) {
        self.margin
            .as_ref()
            .map(|m| m.resolve())
            .unwrap_or((0.0, 0.0, 0.0, 0.0))
    }
}


// --- Card Size (each dimension can be a number or "auto") ---

/// Size dimension: fixed px, "auto", or "50%" (percent of parent)
#[derive(Debug, Clone, JsonSchema)]
pub enum SizeDimension {
    Fixed(f32),
    Percent(f32),
    Auto,
}

impl Serialize for SizeDimension {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            SizeDimension::Fixed(v) => serializer.serialize_f32(*v),
            SizeDimension::Percent(p) => serializer.serialize_str(&format!("{}%", p)),
            SizeDimension::Auto => serializer.serialize_str("auto"),
        }
    }
}

impl<'de> Deserialize<'de> for SizeDimension {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SizeDimensionVisitor;
        impl<'de> serde::de::Visitor<'de> for SizeDimensionVisitor {
            type Value = SizeDimension;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "a number, \"auto\", or \"50%\"")
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<SizeDimension, E> {
                Ok(SizeDimension::Fixed(v as f32))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<SizeDimension, E> {
                Ok(SizeDimension::Fixed(v as f32))
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<SizeDimension, E> {
                Ok(SizeDimension::Fixed(v as f32))
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<SizeDimension, E> {
                if v == "auto" {
                    Ok(SizeDimension::Auto)
                } else if let Some(pct) = v.strip_suffix('%') {
                    pct.trim()
                        .parse::<f32>()
                        .map(SizeDimension::Percent)
                        .map_err(|_| E::custom(format!("invalid percentage: {}", v)))
                } else {
                    Err(E::custom(format!(
                        "expected number, \"auto\", or \"N%\", got: {}",
                        v
                    )))
                }
            }
        }
        deserializer.deserialize_any(SizeDimensionVisitor)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FontStyleType {
    Normal,
    Italic,
    Oblique,
}

impl Default for FontStyleType {
    fn default() -> Self {
        Self::Normal
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TextShadow {
    #[serde(default = "default_shadow_color")]
    pub color: String,
    #[serde(default = "default_shadow_offset")]
    pub offset_x: f32,
    #[serde(default = "default_shadow_offset")]
    pub offset_y: f32,
    #[serde(default = "default_shadow_blur")]
    pub blur: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TextBackground {
    pub color: String,
    #[serde(default = "default_text_bg_padding")]
    pub padding: f32,
    #[serde(default)]
    pub corner_radius: f32,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CaptionWord {
    pub text: String,
    pub start: f64,
    pub end: f64,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum CaptionStyle {
    Highlight,
    Karaoke,
    WordByWord,
}

impl Default for CaptionStyle {
    fn default() -> Self {
        Self::Highlight
    }
}

// --- Shape Text ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ShapeText {
    pub content: String,
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "default_font_family")]
    pub font_family: String,
    #[serde(default)]
    pub font_weight: FontWeight,
    #[serde(default)]
    pub align: TextAlign,
    #[serde(default)]
    pub vertical_align: VerticalAlign,
    #[serde(default)]
    pub line_height: Option<f32>,
    #[serde(default)]
    pub letter_spacing: Option<f32>,
    #[serde(default)]
    pub padding: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VerticalAlign {
    Top,
    Middle,
    Bottom,
}

impl Default for VerticalAlign {
    fn default() -> Self {
        Self::Middle
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CodeblockChrome {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CodeblockHighlight {
    pub lines: Vec<u32>,
    #[serde(default = "default_highlight_color")]
    pub color: String,
    #[serde(default)]
    pub start: Option<f64>,
    #[serde(default)]
    pub end: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CodeblockReveal {
    pub mode: RevealMode,
    #[serde(default)]
    pub start: f64,
    #[serde(default = "default_reveal_duration")]
    pub duration: f64,
    #[serde(default)]
    pub easing: EasingType,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum RevealMode {
    Typewriter,
    LineByLine,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CodeblockState {
    pub code: String,
    pub at: f64,
    #[serde(default = "default_state_duration")]
    pub duration: f64,
    #[serde(default = "default_state_easing")]
    pub easing: EasingType,
    #[serde(default)]
    pub cursor: Option<CodeblockCursor>,
    #[serde(default)]
    pub highlights: Option<Vec<CodeblockHighlight>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CodeblockCursor {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_cursor_color")]
    pub color: String,
    #[serde(default = "default_cursor_width")]
    pub width: f32,
    #[serde(default = "default_true")]
    pub blink: bool,
}

// --- Wiggle Config ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WiggleConfig {
    pub property: String,
    pub amplitude: f64,
    pub frequency: f64,
    #[serde(default)]
    pub seed: u64,
    /// Number of noise octaves (higher = more detail, default 3)
    #[serde(default)]
    pub octaves: Option<u32>,
    /// Phase offset in seconds
    #[serde(default)]
    pub phase: Option<f64>,
    /// Exponential decay rate (amplitude diminishes over time)
    #[serde(default)]
    pub decay: Option<f64>,
    /// Easing function to reshape the noise curve
    #[serde(default)]
    pub easing: Option<EasingType>,
    /// Oscillation mode: "noise" (default, layered simplex) or "sine" (pure sine wave)
    #[serde(default)]
    pub mode: Option<String>,
}

// --- Video Config additions ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    H264,
    H265,
    Vp9,
    Prores,
}

impl Default for VideoCodec {
    fn default() -> Self {
        Self::H264
    }
}

// --- Supporting types ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Size {
    #[serde(default = "default_size_dim")]
    pub width: f32,
    #[serde(default = "default_size_dim")]
    pub height: f32,
}

impl Default for Size {
    fn default() -> Self {
        Self {
            width: 100.0,
            height: 100.0,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ShapeType {
    Rect,
    Circle,
    RoundedRect,
    Ellipse,
    Triangle,
    Star {
        #[serde(default = "default_star_points")]
        points: u32,
    },
    Polygon {
        #[serde(default = "default_polygon_sides")]
        sides: u32,
    },
    Path {
        data: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Fill {
    Solid(String),
    Gradient(Gradient),
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Gradient {
    #[serde(rename = "type")]
    pub gradient_type: GradientType,
    pub colors: Vec<String>,
    #[serde(default)]
    pub stops: Option<Vec<f32>>,
    #[serde(default)]
    pub angle: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GradientType {
    Linear,
    Radial,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Stroke {
    pub color: String,
    #[serde(default = "default_stroke_width")]
    pub width: f32,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ImageFit {
    Cover,
    Contain,
    Fill,
}

impl Default for ImageFit {
    fn default() -> Self {
        Self::Contain
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TextAlign {
    Left,
    Center,
    Right,
}

impl Default for TextAlign {
    fn default() -> Self {
        Self::Left
    }
}

/// Font weight — named ("normal"/"bold") or numeric (100-900)
#[derive(Debug, Clone, JsonSchema)]
pub enum FontWeight {
    Normal,
    Bold,
    Weight(u16),
}

impl Default for FontWeight {
    fn default() -> Self {
        Self::Normal
    }
}

#[allow(dead_code)]
impl FontWeight {
    pub fn to_skia_weight(&self) -> i32 {
        match self {
            FontWeight::Normal => 400,
            FontWeight::Bold => 700,
            FontWeight::Weight(w) => *w as i32,
        }
    }
}

impl Serialize for FontWeight {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            FontWeight::Normal => serializer.serialize_str("normal"),
            FontWeight::Bold => serializer.serialize_str("bold"),
            FontWeight::Weight(w) => serializer.serialize_u16(*w),
        }
    }
}

impl<'de> Deserialize<'de> for FontWeight {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct FontWeightVisitor;
        impl<'de> serde::de::Visitor<'de> for FontWeightVisitor {
            type Value = FontWeight;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "\"normal\", \"bold\", or a number 100-900")
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<FontWeight, E> {
                match v {
                    "normal" => Ok(FontWeight::Normal),
                    "bold" => Ok(FontWeight::Bold),
                    _ => Err(E::custom(format!("unknown font weight: {}", v))),
                }
            }
            fn visit_u64<E: serde::de::Error>(self, v: u64) -> Result<FontWeight, E> {
                Ok(FontWeight::Weight(v as u16))
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<FontWeight, E> {
                Ok(FontWeight::Weight(v as u16))
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<FontWeight, E> {
                Ok(FontWeight::Weight(v as u16))
            }
        }
        deserializer.deserialize_any(FontWeightVisitor)
    }
}

// --- Visual Effect Types ---

/// CSS-like color filter configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FilterConfig {
    #[serde(default)]
    pub brightness: Option<f32>,
    #[serde(default)]
    pub contrast: Option<f32>,
    #[serde(default)]
    pub grayscale: Option<f32>,
    #[serde(default)]
    pub hue_rotate: Option<f32>,
    #[serde(default)]
    pub saturate: Option<f32>,
    #[serde(default)]
    pub sepia: Option<f32>,
}

/// Universal drop shadow
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct DropShadow {
    #[serde(default)]
    pub dx: f32,
    #[serde(default)]
    pub dy: f32,
    #[serde(default = "default_drop_shadow_blur")]
    pub blur: f32,
    #[serde(default = "default_drop_shadow_color")]
    pub color: String,
}

fn default_drop_shadow_blur() -> f32 {
    4.0
}

fn default_drop_shadow_color() -> String {
    "#00000080".to_string()
}

/// Glow effect (colored luminous halo around the element)
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GlowConfig {
    /// Glow color (hex string, e.g. "#5C39EE")
    #[serde(default = "default_glow_color")]
    pub color: String,
    /// Blur radius of the glow
    #[serde(default = "default_glow_radius")]
    pub radius: f32,
    /// Intensity multiplier (higher = brighter glow, default 1.0)
    #[serde(default = "default_glow_intensity")]
    pub intensity: f32,
}

fn default_glow_color() -> String {
    "#FFFFFF80".to_string()
}

fn default_glow_radius() -> f32 {
    10.0
}

fn default_glow_intensity() -> f32 {
    1.0
}

/// Blend mode for layer compositing
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    Multiply,
    Screen,
    Overlay,
    Darken,
    Lighten,
    ColorDodge,
    ColorBurn,
    HardLight,
    SoftLight,
    Difference,
    Exclusion,
    Hue,
    Saturation,
    Color,
    Luminosity,
}

/// Gradient fill for text
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TextGradient {
    pub colors: Vec<String>,
    #[serde(default)]
    pub angle: Option<f32>,
}


// --- Default functions ---

fn default_version() -> String {
    "1.0".to_string()
}

fn default_fps() -> u32 {
    30
}

fn default_background() -> String {
    "#000000".to_string()
}

fn default_font_size() -> f32 {
    48.0
}

fn default_color() -> String {
    "#FFFFFF".to_string()
}

fn default_font_family() -> String {
    "Inter".to_string()
}

fn default_opacity() -> f32 {
    1.0
}

fn default_size_dim() -> f32 {
    100.0
}

fn default_stroke_width() -> f32 {
    2.0
}

fn default_star_points() -> u32 {
    5
}

fn default_polygon_sides() -> u32 {
    6
}



fn default_highlight_color() -> String {
    "#FFFF0033".to_string()
}

fn default_reveal_duration() -> f64 {
    1.0
}

fn default_state_duration() -> f64 {
    0.6
}

fn default_state_easing() -> EasingType {
    EasingType::EaseInOut
}

fn default_true() -> bool {
    true
}

fn default_cursor_color() -> String {
    "#FFFFFF".to_string()
}

fn default_cursor_width() -> f32 {
    2.0
}

fn default_shadow_color() -> String {
    "#00000080".to_string()
}

fn default_shadow_offset() -> f32 {
    2.0
}

fn default_shadow_blur() -> f32 {
    4.0
}

fn default_text_bg_padding() -> f32 {
    8.0
}

fn default_card_border_width() -> f32 {
    1.0
}
