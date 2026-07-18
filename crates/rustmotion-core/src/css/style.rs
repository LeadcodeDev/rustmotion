//! `CssStyle` — typed mirror of the CSS properties supported by the engine.
//!
//! Scope: Remotion-equivalent (Flex, Grid, Block, transforms 2D/3D, filters,
//! gradients, position absolute/relative, box-shadow, border-radius, opacity,
//! clip-path). Excludes: inline boxes, floats, tables, position sticky/fixed.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::units::{Length, LengthPercentage};
use crate::schema::{deserialize_animation_effects, AnimationEffect};

/// Top-level CSS style block. All fields are optional; `None` means "not set"
/// and lets the cascade fill in inherited / initial values.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct CssStyle {
    // ---- Layout / box ----
    pub display: Option<Display>,
    pub position: Option<Position>,
    pub top: Option<LengthPercentage>,
    pub right: Option<LengthPercentage>,
    pub bottom: Option<LengthPercentage>,
    pub left: Option<LengthPercentage>,

    pub width: Option<Size>,
    pub height: Option<Size>,
    pub min_width: Option<Size>,
    pub min_height: Option<Size>,
    pub max_width: Option<Size>,
    pub max_height: Option<Size>,

    pub margin: Option<Edges>,
    pub padding: Option<Edges>,
    pub border: Option<BorderEdges>,
    pub box_sizing: Option<BoxSizing>,
    pub aspect_ratio: Option<f32>,

    // ---- Flex ----
    pub flex_direction: Option<FlexDirection>,
    pub flex_wrap: Option<FlexWrap>,
    pub justify_content: Option<JustifyContent>,
    pub align_items: Option<AlignItems>,
    pub align_self: Option<AlignSelf>,
    pub align_content: Option<AlignContent>,
    pub gap: Option<Gap>,
    pub flex_grow: Option<f32>,
    pub flex_shrink: Option<f32>,
    pub flex_basis: Option<Size>,
    pub order: Option<i32>,

    // ---- Grid ----
    pub grid_template_columns: Option<Vec<GridTrack>>,
    pub grid_template_rows: Option<Vec<GridTrack>>,
    pub grid_column: Option<GridLine>,
    pub grid_row: Option<GridLine>,
    pub grid_auto_flow: Option<GridAutoFlow>,
    pub justify_items: Option<JustifyItems>,
    pub justify_self: Option<JustifySelf>,

    // ---- Typography (most are inherited) ----
    pub font_family: Option<String>,
    pub font_size: Option<Length>,
    pub font_weight: Option<FontWeight>,
    pub font_style: Option<FontStyle>,
    pub line_height: Option<LineHeight>,
    pub letter_spacing: Option<Length>,
    pub text_align: Option<TextAlign>,
    pub color: Option<Color>,
    pub white_space: Option<WhiteSpace>,
    pub overflow_wrap: Option<OverflowWrap>,
    pub text_overflow: Option<TextOverflow>,
    pub text_decoration: Option<TextDecoration>,

    // ---- Visual ----
    pub background: Option<Background>,
    pub border_radius: Option<BorderRadius>,
    pub box_shadow: Option<Vec<BoxShadow>>,
    pub text_shadow: Option<Vec<TextShadow>>,
    pub opacity: Option<f32>,
    pub mix_blend_mode: Option<BlendMode>,
    pub clip_path: Option<ClipPath>,

    // ---- Filters / effects ----
    pub filter: Option<Vec<FilterFn>>,
    pub backdrop_filter: Option<Vec<FilterFn>>,

    // ---- Transform ----
    pub transform: Option<Vec<TransformFn>>,
    pub transform_origin: Option<TransformOrigin>,
    pub perspective: Option<Length>,
    pub perspective_origin: Option<TransformOrigin>,

    // ---- Overflow / stacking ----
    pub overflow: Option<Overflow>,
    pub overflow_x: Option<Overflow>,
    pub overflow_y: Option<Overflow>,
    pub z_index: Option<i32>,
    pub visibility: Option<Visibility>,

    // ---- Animation ----
    #[serde(default, deserialize_with = "deserialize_animation_effects")]
    pub animation: Vec<AnimationEffect>,
}

// ---- Painter convenience accessors ----
//
// These resolve raw CssStyle values to the simple primitives that component
// painters deal with: f32 px, &str hex colors, etc. They drop unsupported
// units (em/rem/% with no parent context). Painters that need full
// resolution should use `Length::resolve(&LengthContext)` directly.
impl CssStyle {
    /// `font-size` in px, falling back to `default` when unset.
    pub fn font_size_px_or(&self, default: f32) -> f32 {
        self.font_size.as_ref().map(|l| l.px()).unwrap_or(default)
    }

    /// `font-size` in px if set as a length.
    pub fn font_size_px(&self) -> Option<f32> {
        self.font_size.as_ref().map(|l| l.px())
    }

    /// `color` as a hex string (only `Color::String` returns Some).
    pub fn color_str(&self) -> Option<&str> {
        match &self.color {
            Some(Color::String(s)) => Some(s.as_str()),
            _ => None,
        }
    }

    /// `color` as a hex string with default fallback.
    pub fn color_str_or<'a>(&'a self, default: &'a str) -> &'a str {
        self.color_str().unwrap_or(default)
    }

    /// `font-family` string.
    pub fn font_family_str(&self) -> Option<&str> {
        self.font_family.as_deref()
    }

    /// `font-family` string with default fallback.
    pub fn font_family_or<'a>(&'a self, default: &'a str) -> &'a str {
        self.font_family.as_deref().unwrap_or(default)
    }

    /// `letter-spacing` in px, defaulting to 0.
    pub fn letter_spacing_px(&self) -> f32 {
        self.letter_spacing.as_ref().map(|l| l.px()).unwrap_or(0.0)
    }

    /// `line-height` resolution. For `Number` (unitless) returns
    /// `n * font_size`; for `Length(px)` returns the px value; otherwise
    /// returns `1.3 * font_size`.
    pub fn line_height_for(&self, font_size: f32) -> f32 {
        match &self.line_height {
            Some(LineHeight::Number(n)) => n * font_size,
            Some(LineHeight::Length(l)) => l.px(),
            _ => font_size * 1.3,
        }
    }

    /// `opacity` with default 1.0.
    pub fn opacity_or(&self, default: f32) -> f32 {
        self.opacity.unwrap_or(default)
    }

    /// `border-radius` resolved as a single uniform px value (drops per-corner).
    pub fn border_radius_px(&self) -> Option<f32> {
        match &self.border_radius {
            Some(BorderRadius::Uniform(lp)) => Some(lp.px()),
            Some(BorderRadius::Corners { top_left, .. }) => Some(top_left.px()),
            None => None,
        }
    }

    /// `border-radius` as px, defaulting to `default`.
    pub fn border_radius_px_or(&self, default: f32) -> f32 {
        self.border_radius_px().unwrap_or(default)
    }

    /// Resolved padding tuple `(top, right, bottom, left)` in px.
    pub fn padding_px(&self) -> (f32, f32, f32, f32) {
        edges_px(self.padding.as_ref())
    }

    /// Resolved margin tuple `(top, right, bottom, left)` in px.
    pub fn margin_px(&self) -> (f32, f32, f32, f32) {
        edges_px(self.margin.as_ref())
    }

    /// `background` as a hex/keyword string when set as a plain color.
    pub fn background_color_str(&self) -> Option<&str> {
        match &self.background {
            Some(Background::Color(Color::String(s))) => Some(s.as_str()),
            _ => None,
        }
    }
}

fn edges_px(e: Option<&Edges>) -> (f32, f32, f32, f32) {
    match e {
        Some(Edges::Uniform(v)) => {
            let p = v.px();
            (p, p, p, p)
        }
        Some(Edges::Sides {
            top,
            right,
            bottom,
            left,
        }) => (top.px(), right.px(), bottom.px(), left.px()),
        None => (0.0, 0.0, 0.0, 0.0),
    }
}

// ---- Layout enums ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Display {
    Block,
    Flex,
    Grid,
    InlineBlock,
    None,
    Contents,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Position {
    Static,
    Relative,
    Absolute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BoxSizing {
    ContentBox,
    BorderBox,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Overflow {
    Visible,
    Hidden,
    Auto,
    Scroll,
    Clip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Visibility {
    Visible,
    Hidden,
}

/// `width: <length>` / `width: auto` / `width: 50%` / `width: max-content` / etc.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Size {
    Auto(AutoKw),
    Length(LengthPercentage),
    Keyword(SizeKeyword),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AutoKw {
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SizeKeyword {
    MaxContent,
    MinContent,
    FitContent,
}

/// Edge values for `margin` / `padding`. Either uniform or per-side.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Edges {
    Uniform(LengthPercentage),
    Sides {
        #[serde(default)]
        top: LengthPercentage,
        #[serde(default)]
        right: LengthPercentage,
        #[serde(default)]
        bottom: LengthPercentage,
        #[serde(default)]
        left: LengthPercentage,
    },
}

impl Edges {
    pub fn resolve(
        &self,
    ) -> (
        LengthPercentage,
        LengthPercentage,
        LengthPercentage,
        LengthPercentage,
    ) {
        match self {
            Edges::Uniform(v) => (v.clone(), v.clone(), v.clone(), v.clone()),
            Edges::Sides {
                top,
                right,
                bottom,
                left,
            } => (top.clone(), right.clone(), bottom.clone(), left.clone()),
        }
    }
}

/// `border: 1px solid red` modeled per side.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct BorderEdges {
    pub width: Option<Edges>,
    pub style: Option<BorderStyle>,
    pub color: Option<Color>,
    /// Per-side overrides.
    pub top: Option<BorderSide>,
    pub right: Option<BorderSide>,
    pub bottom: Option<BorderSide>,
    pub left: Option<BorderSide>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct BorderSide {
    pub width: Option<Length>,
    pub style: Option<BorderStyle>,
    pub color: Option<Color>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BorderStyle {
    None,
    Solid,
    Dashed,
    Dotted,
    Double,
}

/// Border-radius: uniform or per-corner.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum BorderRadius {
    Uniform(LengthPercentage),
    Corners {
        #[serde(default)]
        top_left: LengthPercentage,
        #[serde(default)]
        top_right: LengthPercentage,
        #[serde(default)]
        bottom_right: LengthPercentage,
        #[serde(default)]
        bottom_left: LengthPercentage,
    },
}

// ---- Flex ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FlexDirection {
    Row,
    RowReverse,
    Column,
    ColumnReverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FlexWrap {
    Nowrap,
    Wrap,
    WrapReverse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum JustifyContent {
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AlignItems {
    Stretch,
    FlexStart,
    FlexEnd,
    Center,
    Baseline,
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AlignSelf {
    Auto,
    Stretch,
    FlexStart,
    FlexEnd,
    Center,
    Baseline,
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum AlignContent {
    Stretch,
    FlexStart,
    FlexEnd,
    Center,
    SpaceBetween,
    SpaceAround,
    SpaceEvenly,
    Start,
    End,
}

/// `gap: 8px` (uniform) or `gap: 8px 16px` (row-gap, column-gap).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Gap {
    Uniform(LengthPercentage),
    RowColumn {
        row: LengthPercentage,
        column: LengthPercentage,
    },
}

// ---- Grid ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum GridTrack {
    Length(LengthPercentage),
    Fr(f32),
    Keyword(GridTrackKeyword),
    /// `minmax(min, max)`
    Minmax {
        min: Box<GridTrack>,
        max: Box<GridTrack>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum GridTrackKeyword {
    Auto,
    MinContent,
    MaxContent,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
#[derive(Default)]
pub struct GridLine {
    pub start: Option<GridLineEnd>,
    pub end: Option<GridLineEnd>,
    pub span: Option<u16>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum GridLineEnd {
    Index(i32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum GridAutoFlow {
    Row,
    Column,
    RowDense,
    ColumnDense,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum JustifyItems {
    Stretch,
    Start,
    End,
    Center,
    Legacy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum JustifySelf {
    Auto,
    Stretch,
    Start,
    End,
    Center,
}

// ---- Typography ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum FontWeight {
    Keyword(FontWeightKw),
    Number(u16),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FontWeightKw {
    Normal,
    Bold,
    Bolder,
    Lighter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum FontStyle {
    Normal,
    Italic,
    Oblique,
}

/// `line-height: 1.5` (number) or `line-height: 24px` (length).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum LineHeight {
    Number(f32),
    Length(LengthPercentage),
    Keyword(LineHeightKw),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum LineHeightKw {
    Normal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TextAlign {
    Left,
    Right,
    Center,
    Justify,
    Start,
    End,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum WhiteSpace {
    Normal,
    Nowrap,
    Pre,
    PreLine,
    PreWrap,
    BreakSpaces,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum OverflowWrap {
    Normal,
    BreakWord,
    Anywhere,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TextOverflow {
    Clip,
    Ellipsis,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct TextDecoration {
    pub line: Option<TextDecorationLine>,
    pub style: Option<TextDecorationStyle>,
    pub color: Option<Color>,
    pub thickness: Option<Length>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TextDecorationLine {
    None,
    Underline,
    Overline,
    LineThrough,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum TextDecorationStyle {
    Solid,
    Double,
    Dotted,
    Dashed,
    Wavy,
}

// ---- Color ----

/// Typed color. Strings are parsed lazily ("#rgb", "rgba(..)", named colors).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Color {
    String(String),
    Rgba {
        r: u8,
        g: u8,
        b: u8,
        #[serde(default = "one_f32")]
        a: f32,
    },
}

fn one_f32() -> f32 {
    1.0
}

impl Color {
    /// CSS-string form: pass strings through, format rgba as `#rrggbb[aa]`.
    pub fn to_css_string(&self) -> String {
        match self {
            Color::String(s) => s.clone(),
            Color::Rgba { r, g, b, a } => {
                if *a >= 1.0 {
                    format!("#{r:02x}{g:02x}{b:02x}")
                } else {
                    let alpha = (a.clamp(0.0, 1.0) * 255.0) as u8;
                    format!("#{r:02x}{g:02x}{b:02x}{alpha:02x}")
                }
            }
        }
    }
}

// ---- Background ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Background {
    Color(Color),
    Layers(Vec<BackgroundLayer>),
    Single(BackgroundLayer),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum BackgroundLayer {
    Color {
        color: Color,
    },
    LinearGradient {
        #[serde(default)]
        angle: Option<f32>,
        stops: Vec<GradientStop>,
    },
    RadialGradient {
        #[serde(default)]
        shape: Option<RadialShape>,
        #[serde(default)]
        position: Option<TransformOrigin>,
        stops: Vec<GradientStop>,
    },
    ConicGradient {
        #[serde(default)]
        from: Option<f32>,
        #[serde(default)]
        position: Option<TransformOrigin>,
        stops: Vec<GradientStop>,
    },
    Image {
        url: String,
        #[serde(default)]
        size: Option<BackgroundSize>,
        #[serde(default)]
        position: Option<TransformOrigin>,
        #[serde(default)]
        repeat: Option<BackgroundRepeat>,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct GradientStop {
    pub color: Color,
    pub offset: Option<f32>,
}

impl Default for GradientStop {
    fn default() -> Self {
        Self {
            color: Color::String("#000000".into()),
            offset: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum RadialShape {
    Circle,
    Ellipse,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BackgroundSize {
    Cover,
    Contain,
    Auto,
    Length {
        width: LengthPercentage,
        height: LengthPercentage,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BackgroundRepeat {
    Repeat,
    NoRepeat,
    RepeatX,
    RepeatY,
    Round,
    Space,
}

// ---- Shadows ----

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct BoxShadow {
    pub offset_x: Length,
    pub offset_y: Length,
    pub blur: Option<Length>,
    pub spread: Option<Length>,
    pub color: Option<Color>,
    pub inset: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct TextShadow {
    pub offset_x: Length,
    pub offset_y: Length,
    pub blur: Option<Length>,
    pub color: Option<Color>,
}

impl TextShadow {
    /// Resolve into the legacy schema shadow consumed by the text painters.
    pub fn to_schema(&self, ctx: &crate::css::units::LengthContext) -> crate::schema::TextShadow {
        crate::schema::TextShadow {
            color: self
                .color
                .as_ref()
                .map(Color::to_css_string)
                .unwrap_or_else(|| "#000000".to_string()),
            offset_x: self.offset_x.resolve(ctx),
            offset_y: self.offset_y.resolve(ctx),
            blur: self.blur.as_ref().map(|b| b.resolve(ctx)).unwrap_or(0.0),
        }
    }
}

// ---- Transform ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "fn", rename_all = "kebab-case")]
pub enum TransformFn {
    Translate {
        x: LengthPercentage,
        #[serde(default)]
        y: LengthPercentage,
    },
    TranslateX {
        x: LengthPercentage,
    },
    TranslateY {
        y: LengthPercentage,
    },
    TranslateZ {
        z: Length,
    },
    Translate3d {
        x: LengthPercentage,
        y: LengthPercentage,
        z: Length,
    },
    Scale {
        x: f32,
        #[serde(default = "one_f32")]
        y: f32,
    },
    ScaleX {
        x: f32,
    },
    ScaleY {
        y: f32,
    },
    ScaleZ {
        z: f32,
    },
    Scale3d {
        x: f32,
        y: f32,
        z: f32,
    },
    Rotate {
        deg: f32,
    },
    RotateX {
        deg: f32,
    },
    RotateY {
        deg: f32,
    },
    RotateZ {
        deg: f32,
    },
    Rotate3d {
        x: f32,
        y: f32,
        z: f32,
        deg: f32,
    },
    Skew {
        x: f32,
        #[serde(default)]
        y: f32,
    },
    SkewX {
        x: f32,
    },
    SkewY {
        y: f32,
    },
    Perspective {
        length: Length,
    },
    Matrix {
        values: [f32; 6],
    },
    Matrix3d {
        values: [f32; 16],
    },
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct TransformOrigin {
    pub x: Option<LengthPercentage>,
    pub y: Option<LengthPercentage>,
    pub z: Option<Length>,
}

// ---- Filters ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "fn", rename_all = "kebab-case")]
pub enum FilterFn {
    Blur {
        radius: Length,
    },
    Brightness {
        value: f32,
    },
    Contrast {
        value: f32,
    },
    Saturate {
        value: f32,
    },
    HueRotate {
        deg: f32,
    },
    Grayscale {
        value: f32,
    },
    Invert {
        value: f32,
    },
    Sepia {
        value: f32,
    },
    DropShadow {
        offset_x: Length,
        offset_y: Length,
        #[serde(default)]
        blur: Option<Length>,
        #[serde(default)]
        color: Option<Color>,
    },
    Opacity {
        value: f32,
    },
}

// ---- Blend ----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum BlendMode {
    Normal,
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
    PlusLighter,
}

// ---- Clip-path ----

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ClipPath {
    None,
    Inset {
        top: LengthPercentage,
        right: LengthPercentage,
        bottom: LengthPercentage,
        left: LengthPercentage,
        #[serde(default)]
        radius: Option<BorderRadius>,
    },
    Circle {
        radius: LengthPercentage,
        #[serde(default)]
        origin: Option<TransformOrigin>,
    },
    Ellipse {
        rx: LengthPercentage,
        ry: LengthPercentage,
        #[serde(default)]
        origin: Option<TransformOrigin>,
    },
    Polygon {
        points: Vec<(LengthPercentage, LengthPercentage)>,
    },
    Path {
        d: String,
    },
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_all_none() {
        let s = CssStyle::default();
        assert!(s.display.is_none());
        assert!(s.padding.is_none());
        assert!(s.transform.is_none());
    }

    #[test]
    fn deserialize_basic_flex() {
        let json = r#"{
            "display": "flex",
            "flex-direction": "column",
            "gap": "16px",
            "align-items": "center",
            "padding": "24px"
        }"#;
        let s: CssStyle = serde_json::from_str(json).unwrap();
        assert_eq!(s.display, Some(Display::Flex));
        assert_eq!(s.flex_direction, Some(FlexDirection::Column));
        assert_eq!(s.align_items, Some(AlignItems::Center));
        assert!(matches!(s.padding, Some(Edges::Uniform(_))));
    }

    #[test]
    fn deserialize_per_side_padding() {
        let json = r#"{ "padding": { "top": "10px", "right": "20px", "bottom": "10px", "left": "20px" } }"#;
        let s: CssStyle = serde_json::from_str(json).unwrap();
        assert!(matches!(s.padding, Some(Edges::Sides { .. })));
    }

    #[test]
    fn deserialize_color_variants() {
        let s1: CssStyle = serde_json::from_str(r##"{ "color": "#ff0000" }"##).unwrap();
        let s2: CssStyle =
            serde_json::from_str(r##"{ "color": { "r": 255, "g": 0, "b": 0, "a": 1.0 } }"##)
                .unwrap();
        assert!(matches!(s1.color, Some(Color::String(_))));
        assert!(matches!(s2.color, Some(Color::Rgba { r: 255, .. })));
    }

    #[test]
    fn deserialize_transform_list() {
        let json = r#"{ "transform": [
            { "fn": "translate-x", "x": "10px" },
            { "fn": "scale", "x": 1.5, "y": 1.5 },
            { "fn": "rotate", "deg": 45.0 }
        ]}"#;
        let s: CssStyle = serde_json::from_str(json).unwrap();
        let t = s.transform.expect("transform set");
        assert_eq!(t.len(), 3);
    }

    #[test]
    fn roundtrip_serialization() {
        let original = CssStyle {
            display: Some(Display::Flex),
            opacity: Some(0.5),
            z_index: Some(10),
            ..Default::default()
        };
        let json = serde_json::to_string(&original).unwrap();
        let parsed: CssStyle = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.display, Some(Display::Flex));
        assert_eq!(parsed.opacity, Some(0.5));
        assert_eq!(parsed.z_index, Some(10));
    }
}
