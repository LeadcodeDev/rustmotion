use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::animation::{Animation, AnimationPreset, EasingType, PresetConfig};

/// Common trait for layer types that support animation, timing, wiggle, and motion blur
pub trait LayerProps {
    fn animations(
        &self,
    ) -> (
        &[Animation],
        Option<&AnimationPreset>,
        Option<&PresetConfig>,
    );
    fn timing(&self) -> (Option<f64>, Option<f64>);
    fn wiggle(&self) -> Option<&[WiggleConfig]>;
    fn motion_blur(&self) -> Option<f32>;
    fn padding(&self) -> (f32, f32, f32, f32) {
        (0.0, 0.0, 0.0, 0.0)
    }
    fn margin(&self) -> (f32, f32, f32, f32) {
        (0.0, 0.0, 0.0, 0.0)
    }
}

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
    pub scenes: Vec<Scene>,
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
    pub children: Vec<Layer>,
    #[serde(default)]
    pub transition: Option<Transition>,
    #[serde(default)]
    pub freeze_at: Option<f64>,
    /// Flex layout for automatic layer positioning
    #[serde(default)]
    pub layout: Option<SceneLayout>,
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
pub struct CardShadow {
    pub color: String,
    #[serde(default)]
    pub offset_x: f32,
    #[serde(default)]
    pub offset_y: f32,
    #[serde(default)]
    pub blur: f32,
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
    // Visual effects
    #[serde(default)]
    pub filter: Option<FilterConfig>,
    #[serde(default, rename = "drop-shadow")]
    pub drop_shadow: Option<DropShadow>,
    #[serde(default)]
    pub glow: Option<GlowConfig>,
    #[serde(default, rename = "blend-mode")]
    pub blend_mode: Option<BlendMode>,
    #[serde(default, rename = "clip-path")]
    pub clip_path: Option<String>,
    #[serde(default, rename = "aspect-ratio")]
    pub aspect_ratio: Option<f32>,
    #[serde(default, rename = "text-gradient")]
    pub text_gradient: Option<TextGradient>,
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
            filter: None,
            drop_shadow: None,
            glow: None,
            blend_mode: None,
            clip_path: None,
            aspect_ratio: None,
            text_gradient: None,
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

// --- Layers ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Layer {
    Text(TextLayer),
    Shape(ShapeLayer),
    Image(ImageLayer),
    Group(GroupLayer),
    Svg(SvgLayer),
    Icon(IconLayer),
    Video(VideoLayer),
    Gif(GifLayer),
    Caption(CaptionLayer),
    Codeblock(CodeblockLayer),
    Counter(CounterLayer),
    Card(CardLayer),
    Flex(CardLayer),
    ProgressBar(ProgressBarLayer),
    QrCode(QrCodeLayer),
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TextLayer {
    pub content: String,
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub max_width: Option<f32>,
    #[serde(default)]
    pub style: LayerStyle,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    #[serde(default)]
    pub preset_config: Option<PresetConfig>,
    #[serde(default)]
    pub start_at: Option<f64>,
    #[serde(default)]
    pub end_at: Option<f64>,
    #[serde(default)]
    pub wiggle: Option<Vec<WiggleConfig>>,
    #[serde(default)]
    pub motion_blur: Option<f32>,
}

// --- Counter Layer ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CounterLayer {
    pub from: f64,
    pub to: f64,
    #[serde(default)]
    pub decimals: u8,
    #[serde(default)]
    pub separator: Option<String>,
    #[serde(default)]
    pub prefix: Option<String>,
    #[serde(default)]
    pub suffix: Option<String>,
    #[serde(default)]
    pub easing: EasingType,
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub style: LayerStyle,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    #[serde(default)]
    pub preset_config: Option<PresetConfig>,
    #[serde(default)]
    pub start_at: Option<f64>,
    #[serde(default)]
    pub end_at: Option<f64>,
    #[serde(default)]
    pub wiggle: Option<Vec<WiggleConfig>>,
    #[serde(default)]
    pub motion_blur: Option<f32>,
}

// --- ProgressBar Layer ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ProgressBarLayer {
    pub position: Position,
    #[serde(default)]
    pub style: LayerStyle,
    /// Progress value from 0.0 to 1.0
    #[serde(default = "default_progress")]
    pub progress: f64,
    /// Bar width
    #[serde(default = "default_progress_width")]
    pub width: f32,
    /// Bar height
    #[serde(default = "default_progress_height")]
    pub height: f32,
    /// Background color
    #[serde(default = "default_progress_bg")]
    pub background_color: String,
    /// Fill color
    #[serde(default = "default_progress_fill")]
    pub fill_color: String,
    /// Border radius
    #[serde(default)]
    pub border_radius: f32,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    #[serde(default)]
    pub preset_config: Option<PresetConfig>,
    #[serde(default)]
    pub start_at: Option<f64>,
    #[serde(default)]
    pub end_at: Option<f64>,
    #[serde(default)]
    pub wiggle: Option<Vec<WiggleConfig>>,
    #[serde(default)]
    pub motion_blur: Option<f32>,
}

// --- QrCode Layer ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct QrCodeLayer {
    pub position: Position,
    #[serde(default)]
    pub style: LayerStyle,
    /// Content to encode
    pub content: String,
    /// QR code size (width and height)
    #[serde(default = "default_qr_size")]
    pub size: f32,
    /// Foreground color
    #[serde(default = "default_qr_fg")]
    pub foreground_color: String,
    /// Background color
    #[serde(default = "default_qr_bg")]
    pub background_color: String,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    #[serde(default)]
    pub preset_config: Option<PresetConfig>,
    #[serde(default)]
    pub start_at: Option<f64>,
    #[serde(default)]
    pub end_at: Option<f64>,
    #[serde(default)]
    pub wiggle: Option<Vec<WiggleConfig>>,
    #[serde(default)]
    pub motion_blur: Option<f32>,
}

// --- Card Child wrapper ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CardChild {
    #[serde(flatten)]
    pub layer: Layer,
}

// --- Card Size (each dimension can be a number or "auto") ---

/// Size dimension: fixed px, "auto", or "50%" (percent of parent)
#[derive(Debug, Clone, JsonSchema)]
pub enum SizeDimension {
    Fixed(f32),
    Percent(f32),
    Auto,
}

impl SizeDimension {
    pub fn fixed(&self) -> Option<f32> {
        match self {
            SizeDimension::Fixed(v) => Some(*v),
            _ => None,
        }
    }
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
pub struct CardSize {
    pub width: SizeDimension,
    pub height: SizeDimension,
}

// --- Card Layer ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CardLayer {
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub size: Option<CardSize>,
    #[serde(default)]
    pub children: Vec<CardChild>,
    #[serde(default)]
    pub style: LayerStyle,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    #[serde(default)]
    pub preset_config: Option<PresetConfig>,
    #[serde(default)]
    pub start_at: Option<f64>,
    #[serde(default)]
    pub end_at: Option<f64>,
    #[serde(default)]
    pub wiggle: Option<Vec<WiggleConfig>>,
    #[serde(default)]
    pub motion_blur: Option<f32>,
    /// Stagger delay (seconds) between each child's animation start
    #[serde(default)]
    pub stagger: Option<f64>,
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
pub struct ShapeLayer {
    pub shape: ShapeType,
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub size: Size,
    #[serde(default)]
    pub text: Option<ShapeText>,
    #[serde(default)]
    pub style: LayerStyle,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    #[serde(default)]
    pub preset_config: Option<PresetConfig>,
    #[serde(default)]
    pub start_at: Option<f64>,
    #[serde(default)]
    pub end_at: Option<f64>,
    #[serde(default)]
    pub wiggle: Option<Vec<WiggleConfig>>,
    #[serde(default)]
    pub motion_blur: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ImageLayer {
    pub src: String,
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default)]
    pub fit: ImageFit,
    #[serde(default)]
    pub style: LayerStyle,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    #[serde(default)]
    pub preset_config: Option<PresetConfig>,
    #[serde(default)]
    pub start_at: Option<f64>,
    #[serde(default)]
    pub end_at: Option<f64>,
    #[serde(default)]
    pub wiggle: Option<Vec<WiggleConfig>>,
    #[serde(default)]
    pub motion_blur: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GroupLayer {
    #[serde(default)]
    pub children: Vec<Layer>,
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub style: LayerStyle,
}

// --- Icon Layer (Iconify) ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct IconLayer {
    /// Iconify identifier: "prefix:name" (e.g. "lucide:home", "mdi:account")
    pub icon: String,
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default)]
    pub style: LayerStyle,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    #[serde(default)]
    pub preset_config: Option<PresetConfig>,
    #[serde(default)]
    pub start_at: Option<f64>,
    #[serde(default)]
    pub end_at: Option<f64>,
    #[serde(default)]
    pub wiggle: Option<Vec<WiggleConfig>>,
    #[serde(default)]
    pub motion_blur: Option<f32>,
}

// --- SVG Layer ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct SvgLayer {
    #[serde(default)]
    pub src: Option<String>,
    #[serde(default)]
    pub data: Option<String>,
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default)]
    pub style: LayerStyle,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    #[serde(default)]
    pub preset_config: Option<PresetConfig>,
    #[serde(default)]
    pub start_at: Option<f64>,
    #[serde(default)]
    pub end_at: Option<f64>,
    #[serde(default)]
    pub wiggle: Option<Vec<WiggleConfig>>,
    #[serde(default)]
    pub motion_blur: Option<f32>,
}

// --- Video Layer ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct VideoLayer {
    pub src: String,
    #[serde(default)]
    pub position: Position,
    pub size: Size,
    #[serde(default)]
    pub trim_start: Option<f64>,
    #[serde(default)]
    pub trim_end: Option<f64>,
    #[serde(default)]
    pub playback_rate: Option<f64>,
    #[serde(default)]
    pub fit: ImageFit,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub loop_video: Option<bool>,
    #[serde(default)]
    pub style: LayerStyle,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    #[serde(default)]
    pub preset_config: Option<PresetConfig>,
    #[serde(default)]
    pub start_at: Option<f64>,
    #[serde(default)]
    pub end_at: Option<f64>,
    #[serde(default)]
    pub wiggle: Option<Vec<WiggleConfig>>,
    #[serde(default)]
    pub motion_blur: Option<f32>,
}

// --- GIF Layer ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct GifLayer {
    pub src: String,
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default)]
    pub fit: ImageFit,
    #[serde(default = "default_loop_true")]
    pub loop_gif: bool,
    #[serde(default)]
    pub style: LayerStyle,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    #[serde(default)]
    pub preset_config: Option<PresetConfig>,
    #[serde(default)]
    pub start_at: Option<f64>,
    #[serde(default)]
    pub end_at: Option<f64>,
    #[serde(default)]
    pub wiggle: Option<Vec<WiggleConfig>>,
    #[serde(default)]
    pub motion_blur: Option<f32>,
}

// --- Caption Layer ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CaptionLayer {
    pub words: Vec<CaptionWord>,
    #[serde(default)]
    pub position: Position,
    #[serde(default = "default_active_color")]
    pub active_color: String,
    #[serde(default)]
    pub mode: CaptionStyle,
    #[serde(default)]
    pub max_width: Option<f32>,
    #[serde(default)]
    pub style: LayerStyle,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    #[serde(default)]
    pub preset_config: Option<PresetConfig>,
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

// --- Codeblock Layer ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct CodeblockLayer {
    pub code: String,
    #[serde(default = "default_codeblock_language")]
    pub language: String,
    #[serde(default = "default_codeblock_theme")]
    pub theme: String,
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub size: Option<Size>,
    #[serde(default)]
    pub show_line_numbers: bool,
    #[serde(default)]
    pub chrome: Option<CodeblockChrome>,
    #[serde(default)]
    pub highlights: Vec<CodeblockHighlight>,
    #[serde(default)]
    pub reveal: Option<CodeblockReveal>,
    #[serde(default)]
    pub states: Vec<CodeblockState>,
    #[serde(default)]
    pub style: LayerStyle,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    #[serde(default)]
    pub preset_config: Option<PresetConfig>,
    #[serde(default)]
    pub start_at: Option<f64>,
    #[serde(default)]
    pub end_at: Option<f64>,
    #[serde(default)]
    pub wiggle: Option<Vec<WiggleConfig>>,
    #[serde(default)]
    pub motion_blur: Option<f32>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default, JsonSchema)]
pub struct Position {
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
}

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

// --- SizeDimension with Percent support ---

// --- LayerProps implementations ---

macro_rules! impl_layer_props_standard {
    ($type:ty) => {
        impl LayerProps for $type {
            fn animations(
                &self,
            ) -> (
                &[Animation],
                Option<&AnimationPreset>,
                Option<&PresetConfig>,
            ) {
                (
                    &self.animations,
                    self.preset.as_ref(),
                    self.preset_config.as_ref(),
                )
            }
            fn timing(&self) -> (Option<f64>, Option<f64>) {
                (self.start_at, self.end_at)
            }
            fn wiggle(&self) -> Option<&[WiggleConfig]> {
                self.wiggle.as_deref()
            }
            fn motion_blur(&self) -> Option<f32> {
                self.motion_blur
            }
            fn padding(&self) -> (f32, f32, f32, f32) {
                self.style.padding_resolved()
            }
            fn margin(&self) -> (f32, f32, f32, f32) {
                self.style.margin_resolved()
            }
        }
    };
}

impl_layer_props_standard!(TextLayer);
impl_layer_props_standard!(ShapeLayer);
impl_layer_props_standard!(ImageLayer);
impl_layer_props_standard!(IconLayer);
impl_layer_props_standard!(SvgLayer);
impl_layer_props_standard!(VideoLayer);
impl_layer_props_standard!(GifLayer);
impl_layer_props_standard!(CodeblockLayer);
impl_layer_props_standard!(CardLayer);
impl_layer_props_standard!(ProgressBarLayer);
impl_layer_props_standard!(QrCodeLayer);

impl LayerProps for CaptionLayer {
    fn animations(
        &self,
    ) -> (
        &[Animation],
        Option<&AnimationPreset>,
        Option<&PresetConfig>,
    ) {
        (
            &self.animations,
            self.preset.as_ref(),
            self.preset_config.as_ref(),
        )
    }
    fn timing(&self) -> (Option<f64>, Option<f64>) {
        (None, None)
    }
    fn wiggle(&self) -> Option<&[WiggleConfig]> {
        None
    }
    fn motion_blur(&self) -> Option<f32> {
        None
    }
    fn padding(&self) -> (f32, f32, f32, f32) {
        self.style.padding_resolved()
    }
    fn margin(&self) -> (f32, f32, f32, f32) {
        self.style.margin_resolved()
    }
}

impl LayerProps for CounterLayer {
    fn animations(
        &self,
    ) -> (
        &[Animation],
        Option<&AnimationPreset>,
        Option<&PresetConfig>,
    ) {
        (
            &self.animations,
            self.preset.as_ref(),
            self.preset_config.as_ref(),
        )
    }
    fn timing(&self) -> (Option<f64>, Option<f64>) {
        (self.start_at, None)
    }
    fn wiggle(&self) -> Option<&[WiggleConfig]> {
        self.wiggle.as_deref()
    }
    fn motion_blur(&self) -> Option<f32> {
        self.motion_blur
    }
    fn padding(&self) -> (f32, f32, f32, f32) {
        self.style.padding_resolved()
    }
    fn margin(&self) -> (f32, f32, f32, f32) {
        self.style.margin_resolved()
    }
}

impl LayerProps for GroupLayer {
    fn animations(
        &self,
    ) -> (
        &[Animation],
        Option<&AnimationPreset>,
        Option<&PresetConfig>,
    ) {
        (&[], None, None)
    }
    fn timing(&self) -> (Option<f64>, Option<f64>) {
        (None, None)
    }
    fn wiggle(&self) -> Option<&[WiggleConfig]> {
        None
    }
    fn motion_blur(&self) -> Option<f32> {
        None
    }
    fn padding(&self) -> (f32, f32, f32, f32) {
        self.style.padding_resolved()
    }
    fn margin(&self) -> (f32, f32, f32, f32) {
        self.style.margin_resolved()
    }
}

impl Layer {
    /// Access the LayerStyle for any layer variant
    pub fn style(&self) -> &LayerStyle {
        match self {
            Layer::Text(l) => &l.style,
            Layer::Shape(l) => &l.style,
            Layer::Image(l) => &l.style,
            Layer::Svg(l) => &l.style,
            Layer::Icon(l) => &l.style,
            Layer::Video(l) => &l.style,
            Layer::Gif(l) => &l.style,
            Layer::Caption(l) => &l.style,
            Layer::Codeblock(l) => &l.style,
            Layer::Counter(l) => &l.style,
            Layer::Group(l) => &l.style,
            Layer::Card(l) => &l.style,
            Layer::Flex(l) => &l.style,
            Layer::ProgressBar(l) => &l.style,
            Layer::QrCode(l) => &l.style,
        }
    }

    /// Access LayerProps for any layer variant
    pub fn props(&self) -> &dyn LayerProps {
        match self {
            Layer::Text(l) => l,
            Layer::Shape(l) => l,
            Layer::Image(l) => l,
            Layer::Svg(l) => l,
            Layer::Icon(l) => l,
            Layer::Video(l) => l,
            Layer::Gif(l) => l,
            Layer::Caption(l) => l,
            Layer::Codeblock(l) => l,
            Layer::Counter(l) => l,
            Layer::Group(l) => l,
            Layer::Card(l) => l,
            Layer::Flex(l) => l,
            Layer::ProgressBar(l) => l,
            Layer::QrCode(l) => l,
        }
    }
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

fn default_loop_true() -> bool {
    true
}

fn default_progress() -> f64 {
    0.5
}
fn default_progress_width() -> f32 {
    300.0
}
fn default_progress_height() -> f32 {
    20.0
}
fn default_progress_bg() -> String {
    "#333333".to_string()
}
fn default_progress_fill() -> String {
    "#4CAF50".to_string()
}

fn default_qr_size() -> f32 {
    200.0
}
fn default_qr_fg() -> String {
    "#000000".to_string()
}
fn default_qr_bg() -> String {
    "#FFFFFF".to_string()
}

fn default_active_color() -> String {
    "#FFFF00".to_string()
}

fn default_codeblock_language() -> String {
    "plain".to_string()
}

fn default_codeblock_theme() -> String {
    "base16-ocean.dark".to_string()
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
