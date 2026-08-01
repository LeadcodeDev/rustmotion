use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::video::AnimationEffect;

// --- Card types ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum CardDirection {
    #[default]
    Column,
    Row,
    ColumnReverse,
    RowReverse,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum CardAlign {
    #[serde(alias = "flex-start", alias = "flex_start")]
    #[default]
    Start,
    Center,
    #[serde(alias = "flex-end", alias = "flex_end")]
    End,
    Stretch,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum CardJustify {
    #[serde(alias = "flex-start", alias = "flex_start")]
    #[default]
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
    /// Style state applied from this step's `at` onwards. Properties snap at
    /// `at`, except the ones the component's `style.transition` smooths
    /// (opacity; color on text/counter).
    #[serde(default)]
    pub style: Option<Box<crate::css::CssStyle>>,
}

/// Font weight — named ("normal"/"bold") or numeric (100-900)
#[derive(Debug, Clone, JsonSchema, Default)]
pub enum FontWeight {
    #[default]
    Normal,
    Bold,
    Weight(u16),
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
#[derive(Default)]
pub enum FontStyleType {
    #[default]
    Normal,
    Italic,
    Oblique,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum TextAlign {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum VerticalAlign {
    Top,
    #[default]
    Middle,
    Bottom,
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
