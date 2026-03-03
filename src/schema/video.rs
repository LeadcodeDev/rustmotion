use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::animation::{Animation, AnimationPreset, PresetConfig};

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Scenario {
    #[serde(default = "default_version")]
    pub version: String,
    pub video: VideoConfig,
    #[serde(default)]
    pub audio: Vec<AudioTrack>,
    #[serde(default)]
    pub scenes: Vec<Scene>,
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
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub transition: Option<Transition>,
    #[serde(default)]
    pub freeze_at: Option<f64>,
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

// --- Layers ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Layer {
    Text(TextLayer),
    Shape(ShapeLayer),
    Image(ImageLayer),
    Group(GroupLayer),
    Svg(SvgLayer),
    Video(VideoLayer),
    Gif(GifLayer),
    Caption(CaptionLayer),
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct TextLayer {
    pub content: String,
    #[serde(default)]
    pub position: Position,
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
    pub max_width: Option<f32>,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub line_height: Option<f32>,
    #[serde(default)]
    pub letter_spacing: Option<f32>,
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
pub struct ShapeLayer {
    pub shape: ShapeType,
    #[serde(default)]
    pub position: Position,
    #[serde(default)]
    pub size: Size,
    #[serde(default)]
    pub fill: Option<Fill>,
    #[serde(default)]
    pub stroke: Option<Stroke>,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default)]
    pub corner_radius: Option<f32>,
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
    pub text: Option<ShapeText>,
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
    #[serde(default = "default_opacity")]
    pub opacity: f32,
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
    pub layers: Vec<Layer>,
    #[serde(default)]
    pub position: Position,
    #[serde(default = "default_opacity")]
    pub opacity: f32,
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
    #[serde(default = "default_opacity")]
    pub opacity: f32,
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
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default = "default_volume")]
    pub volume: f32,
    #[serde(default)]
    pub loop_video: Option<bool>,
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
    #[serde(default = "default_opacity")]
    pub opacity: f32,
    #[serde(default = "default_loop_true")]
    pub loop_gif: bool,
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
    #[serde(default = "default_font_size")]
    pub font_size: f32,
    #[serde(default)]
    pub font_family: Option<String>,
    #[serde(default = "default_color")]
    pub color: String,
    #[serde(default = "default_active_color")]
    pub active_color: String,
    #[serde(default)]
    pub background: Option<String>,
    #[serde(default)]
    pub style: CaptionStyle,
    #[serde(default)]
    pub animations: Vec<Animation>,
    #[serde(default)]
    pub preset: Option<AnimationPreset>,
    #[serde(default)]
    pub preset_config: Option<PresetConfig>,
    #[serde(default)]
    pub max_width: Option<f32>,
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

// --- Wiggle Config ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct WiggleConfig {
    pub property: String,
    pub amplitude: f64,
    pub frequency: f64,
    #[serde(default)]
    pub seed: u64,
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

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Fill {
    Solid(String),
    Gradient(Gradient),
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Gradient {
    #[serde(rename = "type")]
    pub gradient_type: GradientType,
    pub colors: Vec<String>,
    #[serde(default)]
    pub stops: Option<Vec<f32>>,
    #[serde(default)]
    pub angle: Option<f32>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum GradientType {
    Linear,
    Radial,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
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

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum FontWeight {
    Normal,
    Bold,
}

impl Default for FontWeight {
    fn default() -> Self {
        Self::Normal
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

fn default_active_color() -> String {
    "#FFFF00".to_string()
}
