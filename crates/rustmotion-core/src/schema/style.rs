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

/// A single step in a component's animation timeline.
/// Triggers a set of animations at a specific time within the scene.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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

const FONT_WEIGHT_MIN: u16 = 100;
const FONT_WEIGHT_MAX: u16 = 900;

/// Font weight — named ("normal"/"bold") or numeric (100-900)
#[derive(Debug, Clone, Default)]
pub enum FontWeight {
    #[default]
    Normal,
    Bold,
    Weight(u16),
}

impl JsonSchema for FontWeight {
    fn schema_name() -> String {
        "FontWeight".to_string()
    }

    fn json_schema(_gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        use schemars::schema::*;

        let keyword_schema: Schema = SchemaObject {
            instance_type: Some(InstanceType::String.into()),
            enum_values: Some(vec!["normal".into(), "bold".into()]),
            ..Default::default()
        }
        .into();

        let numeric_schema: Schema = SchemaObject {
            instance_type: Some(InstanceType::Integer.into()),
            number: Some(Box::new(NumberValidation {
                minimum: Some(FONT_WEIGHT_MIN as f64),
                maximum: Some(FONT_WEIGHT_MAX as f64),
                ..Default::default()
            })),
            ..Default::default()
        }
        .into();

        SchemaObject {
            subschemas: Some(Box::new(SubschemaValidation {
                one_of: Some(vec![keyword_schema, numeric_schema]),
                ..Default::default()
            })),
            metadata: Some(Box::new(Metadata {
                description: Some(format!(
                    "\"normal\", \"bold\", or an integer {FONT_WEIGHT_MIN}-{FONT_WEIGHT_MAX}"
                )),
                ..Default::default()
            })),
            ..Default::default()
        }
        .into()
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

fn font_weight_in_range_or_named_error<E: serde::de::Error>(v: f64) -> Result<FontWeight, E> {
    if v.fract() != 0.0 || v < FONT_WEIGHT_MIN as f64 || v > FONT_WEIGHT_MAX as f64 {
        return Err(E::custom(format!(
            "font weight {v} out of range: expected \"normal\", \"bold\", or an integer \
             {FONT_WEIGHT_MIN}-{FONT_WEIGHT_MAX}"
        )));
    }
    Ok(FontWeight::Weight(v as u16))
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
                font_weight_in_range_or_named_error(v as f64)
            }
            fn visit_i64<E: serde::de::Error>(self, v: i64) -> Result<FontWeight, E> {
                font_weight_in_range_or_named_error(v as f64)
            }
            fn visit_f64<E: serde::de::Error>(self, v: f64) -> Result<FontWeight, E> {
                font_weight_in_range_or_named_error(v)
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

#[cfg(test)]
mod font_weight_tests {
    use super::*;

    #[test]
    fn out_of_range_integer_is_a_named_error_not_a_silent_wraparound() {
        let err = serde_json::from_str::<FontWeight>("70000")
            .expect_err("70000 must not silently wrap to 4464 via `as u16`");
        let msg = err.to_string();
        assert!(
            msg.contains("70000") && msg.contains("100") && msg.contains("900"),
            "error must name the offending value and the valid range, got: {msg}"
        );
    }

    #[test]
    fn fractional_value_is_a_named_error_not_a_silent_floor() {
        let err = serde_json::from_str::<FontWeight>("0.5")
            .expect_err("0.5 must not silently floor to FontWeight::Weight(0)");
        assert!(
            err.to_string().contains("0.5"),
            "error must name the offending value, got: {err}"
        );
    }

    #[test]
    fn in_range_integer_and_keywords_still_parse() {
        assert!(matches!(
            serde_json::from_str::<FontWeight>("700").unwrap(),
            FontWeight::Weight(700)
        ));
        assert!(matches!(
            serde_json::from_str::<FontWeight>(r#""bold""#).unwrap(),
            FontWeight::Bold
        ));
        assert!(matches!(
            serde_json::from_str::<FontWeight>(r#""normal""#).unwrap(),
            FontWeight::Normal
        ));
    }

    #[test]
    fn exported_schema_accepts_the_shapes_the_parser_accepts() {
        let mut generator = schemars::gen::SchemaGenerator::default();
        let schema = <FontWeight as schemars::JsonSchema>::json_schema(&mut generator);
        let json = serde_json::to_value(&schema).unwrap();
        let one_of = json["oneOf"]
            .as_array()
            .expect("FontWeight's schema must be a `oneOf` of [keyword, number]");
        assert_eq!(one_of.len(), 2, "expected exactly 2 branches, got: {json}");
        assert!(
            !json.to_string().contains("\"Weight\""),
            "the externally-tagged `Weight` shape must not leak into the exported schema, got: {json}"
        );
    }
}

#[cfg(test)]
mod timeline_step_tests {
    use super::*;

    #[test]
    fn typo_d_key_is_a_named_error_not_a_silently_inert_step() {
        let err = serde_json::from_value::<TimelineStep>(serde_json::json!({
            "at": 0.2,
            "styel": { "opacity": 0.0 }
        }))
        .expect_err("a typo'd `styel` must not parse into an accepted, inert step");
        assert!(
            err.to_string().contains("styel"),
            "error must name the offending key, got: {err}"
        );
    }

    #[test]
    fn well_formed_step_still_parses() {
        let step: TimelineStep = serde_json::from_value(serde_json::json!({
            "at": 0.2,
            "style": { "opacity": 0.0 }
        }))
        .unwrap();
        assert_eq!(step.at, 0.2);
        assert!(step.style.is_some());
    }
}
