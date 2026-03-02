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
