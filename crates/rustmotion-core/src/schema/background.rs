use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::animation::EasingType;
use super::scenario::default_transition_easing;
use super::video::GradientType;

/// Scroll direction for animated backgrounds.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ScrollDirection {
    Up,
    Down,
    Left,
    Right,
    UpLeft,
    UpRight,
    DownLeft,
    DownRight,
    /// Clockwise rotation (gradient_shift only).
    Cw,
    /// Counter-clockwise rotation (gradient_shift only).
    Ccw,
}

/// Config for the `gradient_shift` preset.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GradientShiftConfig {
    pub colors: Vec<String>,
    #[serde(default = "default_bg_type")]
    pub gradient_type: GradientType,
}

/// Config for the `grid_dots` preset.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GridDotsConfig {
    #[serde(default = "default_grid_dots_color")]
    pub color: String,
    #[serde(default = "default_bg_element_size")]
    pub element_size: f32,
    #[serde(default = "default_bg_spacing")]
    pub spacing: f32,
}

fn default_grid_dots_color() -> String {
    "#FFFFFF15".to_string()
}

/// Config for the `concentric_circles` preset.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ConcentricCirclesConfig {
    #[serde(default = "default_concentric_color")]
    pub color: String,
    #[serde(default = "default_bg_element_size")]
    pub element_size: f32,
    #[serde(default = "default_bg_spacing")]
    pub spacing: f32,
    #[serde(default)]
    pub count: Option<u32>,
}

fn default_concentric_color() -> String {
    "#FFFFFF20".to_string()
}

/// Config for the `halo` preset.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HaloConfig {
    pub zones: Vec<HaloZone>,
}

/// Config for the `heropattern` preset.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HeropatternConfig {
    /// Name of the heropattern (e.g. "plus", "topography", "jigsaw").
    pub pattern: String,
    #[serde(default = "default_hero_color")]
    pub color: String,
    #[serde(default = "default_hero_opacity")]
    pub opacity: f32,
    #[serde(default = "default_hero_scale")]
    pub scale: f32,
}

fn default_hero_color() -> String {
    "#FFFFFF".to_string()
}

fn default_hero_opacity() -> f32 {
    0.1
}

fn default_hero_scale() -> f32 {
    1.0
}

/// Typed background preset with its config.
#[derive(Debug, Clone)]
pub enum BackgroundPreset {
    GradientShift(GradientShiftConfig),
    GridDots(GridDotsConfig),
    ConcentricCircles(ConcentricCirclesConfig),
    Halo(HaloConfig),
    Heropattern(HeropatternConfig),
}

impl BackgroundPreset {
    pub fn name(&self) -> &'static str {
        match self {
            BackgroundPreset::GradientShift(_) => "gradient_shift",
            BackgroundPreset::GridDots(_) => "grid_dots",
            BackgroundPreset::ConcentricCircles(_) => "concentric_circles",
            BackgroundPreset::Halo(_) => "halo",
            BackgroundPreset::Heropattern(_) => "heropattern",
        }
    }
}

/// Animated background configuration for scenes.
#[derive(Debug, Clone)]
pub struct AnimatedBackground {
    pub preset: BackgroundPreset,
    /// Horizontal offset (pixels).
    pub x: f32,
    /// Vertical offset (pixels).
    pub y: f32,
    /// Animation speed (px/sec for tiled presets, deg/sec for gradient_shift).
    pub speed: f32,
    /// Scroll direction.
    pub direction: Option<ScrollDirection>,
}

impl Serialize for AnimatedBackground {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = serializer.serialize_map(None)?;
        map.serialize_entry("preset", self.preset.name())?;
        // Serialize preset-specific config under its name key
        match &self.preset {
            BackgroundPreset::GradientShift(cfg) => map.serialize_entry("gradient_shift", cfg)?,
            BackgroundPreset::GridDots(cfg) => map.serialize_entry("grid_dots", cfg)?,
            BackgroundPreset::ConcentricCircles(cfg) => {
                map.serialize_entry("concentric_circles", cfg)?
            }
            BackgroundPreset::Halo(cfg) => map.serialize_entry("halo", cfg)?,
            BackgroundPreset::Heropattern(cfg) => map.serialize_entry("heropattern", cfg)?,
        }
        map.serialize_entry("speed", &self.speed)?;
        if self.x != 0.0 {
            map.serialize_entry("x", &self.x)?;
        }
        if self.y != 0.0 {
            map.serialize_entry("y", &self.y)?;
        }
        if let Some(ref dir) = self.direction {
            map.serialize_entry("direction", dir)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for AnimatedBackground {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let map: serde_json::Map<String, serde_json::Value> =
            serde_json::Map::deserialize(deserializer)?;

        // Common fields
        let x = map.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let y = map.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let direction: Option<ScrollDirection> = map
            .get("direction")
            .and_then(|v| serde_json::from_value(v.clone()).ok());

        let preset_str = map.get("preset").and_then(|v| v.as_str()).unwrap_or("");

        // Detect new vs legacy format: new format has a sub-object keyed by preset name
        let is_new_format =
            !preset_str.is_empty() && map.get(preset_str).map_or(false, |v| v.is_object());

        let (preset, speed) = if is_new_format {
            // New format: config in sub-object
            let sub = map.get(preset_str).unwrap().clone();
            let speed = map.get("speed").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let preset = match preset_str {
                "grid_dots" => {
                    let cfg: GridDotsConfig =
                        serde_json::from_value(sub).map_err(serde::de::Error::custom)?;
                    BackgroundPreset::GridDots(cfg)
                }
                "concentric_circles" => {
                    let cfg: ConcentricCirclesConfig =
                        serde_json::from_value(sub).map_err(serde::de::Error::custom)?;
                    BackgroundPreset::ConcentricCircles(cfg)
                }
                "halo" => {
                    let cfg: HaloConfig =
                        serde_json::from_value(sub).map_err(serde::de::Error::custom)?;
                    BackgroundPreset::Halo(cfg)
                }
                "heropattern" => {
                    let cfg: HeropatternConfig =
                        serde_json::from_value(sub).map_err(serde::de::Error::custom)?;
                    BackgroundPreset::Heropattern(cfg)
                }
                _ => {
                    // gradient_shift or unknown → gradient_shift
                    let cfg: GradientShiftConfig =
                        serde_json::from_value(sub).map_err(serde::de::Error::custom)?;
                    BackgroundPreset::GradientShift(cfg)
                }
            };
            (preset, speed)
        } else {
            // Legacy flat format
            let legacy_speed = map.get("speed").and_then(|v| v.as_f64()).unwrap_or(30.0) as f32;
            let preset = match preset_str {
                "grid_dots" => {
                    let color = map
                        .get("colors")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.first())
                        .and_then(|v| v.as_str())
                        .unwrap_or("#FFFFFF15")
                        .to_string();
                    let element_size = map
                        .get("element_size")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(4.0) as f32;
                    let spacing =
                        map.get("spacing").and_then(|v| v.as_f64()).unwrap_or(60.0) as f32;
                    BackgroundPreset::GridDots(GridDotsConfig {
                        color,
                        element_size,
                        spacing,
                    })
                }
                "concentric_circles" => {
                    let color = map
                        .get("colors")
                        .and_then(|v| v.as_array())
                        .and_then(|a| a.first())
                        .and_then(|v| v.as_str())
                        .unwrap_or("#FFFFFF20")
                        .to_string();
                    let element_size = map
                        .get("element_size")
                        .and_then(|v| v.as_f64())
                        .unwrap_or(4.0) as f32;
                    let spacing =
                        map.get("spacing").and_then(|v| v.as_f64()).unwrap_or(60.0) as f32;
                    let count = map.get("count").and_then(|v| v.as_u64()).map(|n| n as u32);
                    BackgroundPreset::ConcentricCircles(ConcentricCirclesConfig {
                        color,
                        element_size,
                        spacing,
                        count,
                    })
                }
                "halo" => {
                    let zones: Vec<HaloZone> = map
                        .get("zones")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or_default();
                    BackgroundPreset::Halo(HaloConfig { zones })
                }
                _ => {
                    // Default: gradient_shift
                    let colors: Vec<String> = map
                        .get("colors")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or_default();
                    let gradient_type: GradientType = map
                        .get("gradient_type")
                        .and_then(|v| serde_json::from_value(v.clone()).ok())
                        .unwrap_or_else(default_bg_type);
                    BackgroundPreset::GradientShift(GradientShiftConfig {
                        colors,
                        gradient_type,
                    })
                }
            };
            (preset, legacy_speed)
        };

        // Infer legacy direction if not specified
        let direction = direction.or_else(|| {
            if speed > 0.0 && !is_new_format {
                match &preset {
                    BackgroundPreset::GradientShift(_) => Some(ScrollDirection::Cw),
                    BackgroundPreset::GridDots(_) => Some(ScrollDirection::Up),
                    _ => None,
                }
            } else {
                None
            }
        });

        Ok(AnimatedBackground {
            preset,
            x,
            y,
            speed,
            direction,
        })
    }
}

impl JsonSchema for AnimatedBackground {
    fn schema_name() -> String {
        "AnimatedBackground".to_string()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        use schemars::schema::*;

        let mut props = schemars::Map::new();
        props.insert("preset".to_string(), gen.subschema_for::<String>());
        props.insert("x".to_string(), gen.subschema_for::<f32>());
        props.insert("y".to_string(), gen.subschema_for::<f32>());
        props.insert("speed".to_string(), gen.subschema_for::<f32>());
        props.insert(
            "direction".to_string(),
            gen.subschema_for::<Option<ScrollDirection>>(),
        );
        props.insert(
            "gradient_shift".to_string(),
            gen.subschema_for::<Option<GradientShiftConfig>>(),
        );
        props.insert(
            "grid_dots".to_string(),
            gen.subschema_for::<Option<GridDotsConfig>>(),
        );
        props.insert(
            "concentric_circles".to_string(),
            gen.subschema_for::<Option<ConcentricCirclesConfig>>(),
        );
        props.insert(
            "halo".to_string(),
            gen.subschema_for::<Option<HaloConfig>>(),
        );
        props.insert(
            "heropattern".to_string(),
            gen.subschema_for::<Option<HeropatternConfig>>(),
        );

        SchemaObject {
            instance_type: Some(InstanceType::Object.into()),
            object: Some(Box::new(ObjectValidation {
                properties: props,
                required: ["preset".to_string()].into_iter().collect(),
                ..Default::default()
            })),
            ..Default::default()
        }
        .into()
    }
}

/// A single glow zone for the "halo" animated-background preset.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct HaloZone {
    /// Zone color (hex string).
    pub color: String,
    /// X position as fraction of width (0.0 = left, 1.0 = right).
    #[serde(default = "default_half")]
    pub x: f32,
    /// Y position as fraction of height (0.0 = top, 1.0 = bottom).
    #[serde(default = "default_half")]
    pub y: f32,
    /// Radius as fraction of max(width, height).
    #[serde(default = "default_halo_radius")]
    pub radius: f32,
}

/// Transition configuration for background interpolation between scenes.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct BackgroundTransition {
    pub duration: f64,
    #[serde(default = "default_transition_easing")]
    pub easing: EasingType,
}

/// A background entry with optional template reference and transition.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundEntry {
    #[serde(rename = "$ref", default)]
    pub template_ref: Option<String>,
    #[serde(default)]
    pub transition: Option<BackgroundTransition>,
    #[serde(flatten)]
    pub overrides: serde_json::Map<String, serde_json::Value>,
}

/// The unified background field: color string, single entry, or multiple entries.
#[derive(Debug, Clone)]
pub enum BackgroundValue {
    Color(String),
    Single(BackgroundEntry),
    Multiple(Vec<BackgroundEntry>),
}

impl Serialize for BackgroundValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            BackgroundValue::Color(s) => serializer.serialize_str(s),
            BackgroundValue::Single(entry) => entry.serialize(serializer),
            BackgroundValue::Multiple(entries) => entries.serialize(serializer),
        }
    }
}

/// Resolved background after template expansion — ready for rendering.
#[derive(Debug, Clone, Default, Serialize)]
pub struct ResolvedBackground {
    pub color: Option<String>,
    pub animated: Vec<AnimatedBackground>,
    /// Transition for interpolation from the previous scene's background.
    pub transition: Option<BackgroundTransition>,
}

fn default_half() -> f32 {
    0.5
}

fn default_halo_radius() -> f32 {
    0.4
}

fn default_bg_element_size() -> f32 {
    4.0
}

fn default_bg_spacing() -> f32 {
    60.0
}

#[allow(dead_code)]
fn default_bg_speed() -> f32 {
    30.0
}

fn default_bg_type() -> GradientType {
    GradientType::Linear
}

/// Deserialize `animated-background` as either a single AnimatedBackground or a Vec.
pub(crate) fn deserialize_animated_backgrounds<'de, D>(
    deserializer: D,
) -> Result<Vec<AnimatedBackground>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct OneOrMany;

    impl<'de> de::Visitor<'de> for OneOrMany {
        type Value = Vec<AnimatedBackground>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a single animated background or an array of animated backgrounds")
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
            let bg = AnimatedBackground::deserialize(de::value::MapAccessDeserializer::new(map))?;
            Ok(vec![bg])
        }
    }

    deserializer.deserialize_any(OneOrMany)
}

/// Deserialize `background` as a color string, a single BackgroundEntry object, or an array.
pub(crate) fn deserialize_background_value<'de, D>(
    deserializer: D,
) -> Result<Option<BackgroundValue>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de;

    struct BgVisitor;

    impl<'de> de::Visitor<'de> for BgVisitor {
        type Value = Option<BackgroundValue>;

        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            f.write_str("a color string, a background object, or an array of background objects")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(BackgroundValue::Color(v.to_string())))
        }

        fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(Some(BackgroundValue::Color(v)))
        }

        fn visit_map<M>(self, map: M) -> Result<Self::Value, M::Error>
        where
            M: de::MapAccess<'de>,
        {
            let entry = BackgroundEntry::deserialize(de::value::MapAccessDeserializer::new(map))?;
            Ok(Some(BackgroundValue::Single(entry)))
        }

        fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            let entries =
                Vec::<BackgroundEntry>::deserialize(de::value::SeqAccessDeserializer::new(seq))?;
            Ok(Some(BackgroundValue::Multiple(entries)))
        }
    }

    deserializer.deserialize_any(BgVisitor)
}
