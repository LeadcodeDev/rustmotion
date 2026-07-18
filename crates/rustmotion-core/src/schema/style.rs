use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::video::{
    AnimationEffect, BlendMode, CardBorder, CardShadow, DropShadow, Fill, FilterConfig,
    GradientBorder, InnerShadow, Stroke, TextBackground, TextGradient, TextShadow,
};

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
    #[serde(alias = "flex-start", alias = "flex_start")]
    Start,
    Center,
    #[serde(alias = "flex-end", alias = "flex_end")]
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
    #[serde(alias = "flex-start", alias = "flex_start")]
    Start,
    Center,
    #[serde(alias = "flex-end", alias = "flex_end")]
    End,
    #[serde(alias = "space-between")]
    SpaceBetween,
    #[serde(alias = "space-around")]
    SpaceAround,
    #[serde(alias = "space-evenly")]
    SpaceEvenly,
}

impl Default for CardJustify {
    fn default() -> Self {
        Self::Start
    }
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

/// A single step in a component's animation timeline.
/// Triggers a set of animations at a specific time within the scene.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct TimelineStep {
    /// Time (in seconds, relative to component start) when this step begins.
    pub at: f64,
    /// Animation effects to apply during this step.
    #[serde(default, deserialize_with = "deserialize_animation_effects")]
    pub animation: Vec<AnimationEffect>,
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

/// CSS-like overflow: controls whether children that exceed this container's
/// bounds are clipped (`hidden`) or allowed to bleed out (`visible`, default).
/// Validation only fails when content escapes the *viewport*; escaping a
/// `visible` parent is legitimate (e.g. a badge sticking out of a card).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Overflow {
    Visible,
    Hidden,
}

impl Default for Overflow {
    fn default() -> Self {
        Self::Visible
    }
}

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

/// Deserialize `animation` as either a single AnimationEffect or a Vec.
pub fn deserialize_animation_effects<'de, D>(
    deserializer: D,
) -> Result<Vec<AnimationEffect>, D::Error>
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
            let effect = AnimationEffect::deserialize(de::value::MapAccessDeserializer::new(map))?;
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
    /// Wrap text content at the available width. None = use sensible default
    /// per component (true for paragraphs, false for atomic labels like counter,
    /// kbd, badge). Explicit `false` makes the geometry validator fail if the
    /// natural width exceeds the constraint.
    #[serde(default)]
    pub wrap: Option<bool>,
    /// CSS-like overflow on containers: `visible` (default) lets children bleed
    /// out, `hidden` clips them. Viewport overflow is always a validation error
    /// regardless of this property.
    #[serde(default)]
    pub overflow: Option<Overflow>,
    // Stagger: automatic delay offset per child in a container (seconds)
    #[serde(default)]
    pub stagger: Option<f32>,
    // Animation effects (accepts single object or array)
    #[serde(default, deserialize_with = "deserialize_animation_effects")]
    pub animation: Vec<AnimationEffect>,
    // Timeline: sequence of animation steps triggered at specific times.
    // Each step has an `at` time and its own animation effects.
    // When present, animations are resolved per-step instead of all at once.
    #[serde(default)]
    pub timeline: Vec<TimelineStep>,
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
            timeline: Vec::new(),
            wrap: None,
            overflow: None,
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

fn default_opacity() -> f32 {
    1.0
}
