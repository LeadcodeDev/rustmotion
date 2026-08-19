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

/// Config for the `grid_lines` preset — a ruled grid, not a dotted one.
///
/// `grid_dots` marks the intersections and reads as texture; ruled lines read
/// as structure, which is what a SaaS/data scene wants behind a chart or a
/// code panel. Same scroll machinery (`x`/`y`/`speed`/`direction`) as the
/// other tiled presets.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GridLinesConfig {
    /// Line colour (hex, alpha welcome).
    #[serde(default = "default_grid_lines_color")]
    pub color: String,
    /// Cell edge in px. Small reads as graph paper, large as panels.
    #[serde(default = "default_grid_lines_cell")]
    pub cell: f32,
    /// Line thickness in px.
    #[serde(default = "default_grid_lines_weight")]
    pub weight: f32,
    /// Draw every Nth line at `major_weight` instead, for the
    /// graph-paper look where the coarse grid reads through the fine one.
    /// `0` (default) means no major lines.
    #[serde(default)]
    pub major_every: u32,
    /// Thickness of a major line.
    #[serde(default = "default_grid_lines_major_weight")]
    pub major_weight: f32,
}

fn default_grid_lines_color() -> String {
    "#FFFFFF14".to_string()
}

fn default_grid_lines_cell() -> f32 {
    72.0
}

fn default_grid_lines_weight() -> f32 {
    1.0
}

fn default_grid_lines_major_weight() -> f32 {
    2.0
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

/// Config for the `pixel_grid` preset: a lattice of square cells.
///
/// Covers two looks with one shape. `density: 1.0` with two colours gives a
/// true checkerboard (cells alternate by `(row + col)` parity); a density
/// below 1 with one colour gives the sparse tile field the reference piece
/// uses — squares on a ground, some cells simply absent.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct PixelGridConfig {
    /// Cell colours. One colour fills every drawn cell; several alternate by
    /// `(row + col)`, which is what makes a checkerboard rather than a field.
    #[serde(default = "default_pixel_colors")]
    pub colors: Vec<String>,
    /// Edge of a cell in px.
    #[serde(default = "default_pixel_size")]
    pub size: f32,
    /// Lattice pitch in px — the distance between two cell origins. Clamped to
    /// at least `size`, so cells never overlap; `spacing - size` is the gap.
    #[serde(default = "default_pixel_spacing")]
    pub spacing: f32,
    /// Fraction of cells drawn, 0..1. Which cells is decided by a hash of the
    /// cell's coordinates, so the pattern is stable from frame to frame — a
    /// per-frame random would boil.
    #[serde(default = "default_pixel_density")]
    pub density: f32,
    /// Where the field is densest. The reference piece ramps its density
    /// across the frame rather than scattering uniformly, which is what stops
    /// the texture reading as noise.
    #[serde(default)]
    pub density_ramp: PixelDensityRamp,
    /// Corner radius of a cell in px. `0` for hard pixels.
    #[serde(default)]
    pub radius: f32,
    /// Stable pattern selector: two backgrounds with the same seed and
    /// geometry are identical, different seeds are different scatters.
    #[serde(default = "default_pixel_seed")]
    pub seed: u32,
    /// How the field moves. `speed` on the background scales it.
    #[serde(default)]
    pub motion: PixelGridMotion,
}

/// Which way the fill density ramps across the frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PixelDensityRamp {
    /// Uniform: every cell has the same chance of being drawn.
    #[default]
    None,
    Left,
    Right,
    Top,
    Bottom,
    /// Dense at the centre, thinning outwards.
    Radial,
    /// Dense at the frame's edges, thinning toward the centre — a vignette.
    ///
    /// Measured on the reference piece, in a band clear of its window, the
    /// density runs 10.9 · 6.5 · 0.8 · 0.2 · 0.2 · 0.2 · 0.2 · 0.9 · 7.8 · 8.8 %
    /// across the tenths of the frame: heavy at both edges, effectively empty
    /// through the middle 60 %. That is what keeps the texture off whatever
    /// sits in the centre, and `Radial` is its exact inverse.
    Edges,
}

/// How a `pixel_grid` animates.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PixelGridMotion {
    /// Still. The lattice is a texture, not an effect.
    #[default]
    None,
    /// Cells fade in and out on their own phase.
    Twinkle,
    /// A band of extra density travels across the field.
    Sweep,
}

fn default_pixel_colors() -> Vec<String> {
    vec!["#FFFFFF22".to_string()]
}

fn default_pixel_size() -> f32 {
    10.0
}

fn default_pixel_spacing() -> f32 {
    24.0
}

fn default_pixel_density() -> f32 {
    0.6
}

fn default_pixel_seed() -> u32 {
    7
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
    GridLines(GridLinesConfig),
    ConcentricCircles(ConcentricCirclesConfig),
    Halo(HaloConfig),
    PixelGrid(PixelGridConfig),
    Heropattern(HeropatternConfig),
}

impl BackgroundPreset {
    pub fn name(&self) -> &'static str {
        match self {
            BackgroundPreset::GradientShift(_) => "gradient_shift",
            BackgroundPreset::GridDots(_) => "grid_dots",
            BackgroundPreset::GridLines(_) => "grid_lines",
            BackgroundPreset::ConcentricCircles(_) => "concentric_circles",
            BackgroundPreset::Halo(_) => "halo",
            BackgroundPreset::PixelGrid(_) => "pixel_grid",
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
            BackgroundPreset::GridLines(cfg) => map.serialize_entry("grid_lines", cfg)?,
            BackgroundPreset::ConcentricCircles(cfg) => {
                map.serialize_entry("concentric_circles", cfg)?
            }
            BackgroundPreset::Halo(cfg) => map.serialize_entry("halo", cfg)?,
            BackgroundPreset::PixelGrid(cfg) => map.serialize_entry("pixel_grid", cfg)?,
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

/// Every preset name the engine actually recognises. A `preset` value
/// outside this list — including the empty string produced when the key is
/// missing entirely — is rejected below instead of silently becoming
/// `gradient_shift` (constat #3, sink 1).
const KNOWN_BACKGROUND_PRESETS: &[&str] = &[
    "gradient_shift",
    "grid_dots",
    "grid_lines",
    "concentric_circles",
    "halo",
    "pixel_grid",
    "heropattern",
];

impl<'de> Deserialize<'de> for AnimatedBackground {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let map: serde_json::Map<String, serde_json::Value> =
            serde_json::Map::deserialize(deserializer)?;

        // Common fields
        let x = map.get("x").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        let y = map.get("y").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
        // Constat #3 (related sink, fixed alongside): a mistyped `direction`
        // used to be swallowed by `.ok()` into a silent `None` — same class
        // as the preset/zones/colors sinks below, just on a smaller field.
        let direction: Option<ScrollDirection> = match map.get("direction") {
            Some(v) => Some(serde_json::from_value(v.clone()).map_err(|e| {
                serde::de::Error::custom(format!("animated-background.direction: {e}"))
            })?),
            None => None,
        };

        let preset_str = map.get("preset").and_then(|v| v.as_str()).unwrap_or("");
        if !KNOWN_BACKGROUND_PRESETS.contains(&preset_str) {
            return Err(serde::de::Error::custom(format!(
                "unknown animated-background preset '{preset_str}': expected one of {}",
                KNOWN_BACKGROUND_PRESETS.join(", ")
            )));
        }

        // Detect new vs legacy format: new format has a sub-object keyed by preset name
        let is_new_format = map.get(preset_str).is_some_and(|v| v.is_object());

        let (preset, speed) = if is_new_format {
            // New format: config in sub-object
            let sub = map.get(preset_str).unwrap().clone();
            let speed = map.get("speed").and_then(|v| v.as_f64()).unwrap_or(0.0) as f32;
            let preset = deserialize_preset_config::<D::Error>(preset_str, sub)?;
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
                    // Constat #3, sink 2: was `.ok().unwrap_or_default()` —
                    // a malformed (or entirely missing) `zones` silently
                    // became an empty halo instead of erroring. Route
                    // through the same validated-struct path as the
                    // new-format branch: `HaloConfig::zones` is required
                    // (no `#[serde(default)]`), so a missing/malformed value
                    // now produces a real "missing/invalid field zones"
                    // error instead.
                    let mut obj = serde_json::Map::new();
                    if let Some(z) = map.get("zones") {
                        obj.insert("zones".to_string(), z.clone());
                    }
                    let cfg: HaloConfig = serde_json::from_value(serde_json::Value::Object(obj))
                        .map_err(|e| {
                            serde::de::Error::custom(format!("animated-background.zones: {e}"))
                        })?;
                    BackgroundPreset::Halo(cfg)
                }
                "heropattern" => {
                    // Constat #3 (related sink, fixed alongside): the legacy
                    // branch never had an arm for `heropattern` at all, so a
                    // *correctly spelled* `"preset": "heropattern"` written
                    // in the legacy flat form (no `heropattern: {...}`
                    // sub-object) fell through the old `_ =>` wildcard and
                    // silently became `gradient_shift` with `colors: []`.
                    let mut obj = serde_json::Map::new();
                    for key in ["pattern", "color", "opacity", "scale"] {
                        if let Some(v) = map.get(key) {
                            obj.insert(key.to_string(), v.clone());
                        }
                    }
                    let cfg: HeropatternConfig =
                        serde_json::from_value(serde_json::Value::Object(obj)).map_err(|e| {
                            serde::de::Error::custom(format!(
                                "animated-background.heropattern: {e}"
                            ))
                        })?;
                    BackgroundPreset::Heropattern(cfg)
                }
                "gradient_shift" => {
                    // Constat #3, sink 3: `colors`/`gradient_type` were each
                    // parsed with `.ok().unwrap_or_default()` /
                    // `.ok().unwrap_or_else(default_bg_type)` — so even with
                    // `preset` spelled *correctly*, a missing or malformed
                    // `colors` silently produced `colors: []`, i.e. a fully
                    // empty gradient that paints black with no diagnostic at
                    // all — the exact worst-case symptom the audit names.
                    let mut obj = serde_json::Map::new();
                    if let Some(c) = map.get("colors") {
                        obj.insert("colors".to_string(), c.clone());
                    }
                    if let Some(g) = map.get("gradient_type") {
                        obj.insert("gradient_type".to_string(), g.clone());
                    }
                    let cfg: GradientShiftConfig =
                        serde_json::from_value(serde_json::Value::Object(obj)).map_err(|e| {
                            serde::de::Error::custom(format!(
                                "animated-background.colors/gradient_type: {e}"
                            ))
                        })?;
                    BackgroundPreset::GradientShift(cfg)
                }
                // Unreachable: `preset_str` was already checked against
                // `KNOWN_BACKGROUND_PRESETS` above.
                other => {
                    return Err(serde::de::Error::custom(format!(
                        "internal error: unhandled animated-background preset '{other}'"
                    )))
                }
            };
            (preset, legacy_speed)
        };

        // Infer legacy direction if not specified
        let direction = direction.or({
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

/// Deserialize the preset-specific config object for the "new" nested
/// format (`{"preset": "halo", "halo": {...}}`) — shared by
/// `AnimatedBackground::deserialize` and available for reuse. `preset_str`
/// must already be one of [`KNOWN_BACKGROUND_PRESETS`].
fn deserialize_preset_config<E: serde::de::Error>(
    preset_str: &str,
    sub: serde_json::Value,
) -> Result<BackgroundPreset, E> {
    match preset_str {
        "grid_dots" => Ok(BackgroundPreset::GridDots(
            serde_json::from_value(sub).map_err(E::custom)?,
        )),
        "grid_lines" => Ok(BackgroundPreset::GridLines(
            serde_json::from_value(sub).map_err(E::custom)?,
        )),
        "concentric_circles" => Ok(BackgroundPreset::ConcentricCircles(
            serde_json::from_value(sub).map_err(E::custom)?,
        )),
        "halo" => Ok(BackgroundPreset::Halo(
            serde_json::from_value(sub).map_err(E::custom)?,
        )),
        "pixel_grid" => Ok(BackgroundPreset::PixelGrid(
            serde_json::from_value(sub).map_err(E::custom)?,
        )),
        "heropattern" => Ok(BackgroundPreset::Heropattern(
            serde_json::from_value(sub).map_err(E::custom)?,
        )),
        "gradient_shift" => Ok(BackgroundPreset::GradientShift(
            serde_json::from_value(sub).map_err(E::custom)?,
        )),
        other => Err(E::custom(format!(
            "internal error: unhandled animated-background preset '{other}'"
        ))),
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
            "grid_lines".to_string(),
            gen.subschema_for::<Option<GridLinesConfig>>(),
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
    /// Zone color (hex string). May itself carry an alpha channel
    /// (`#rrggbbaa`); see [`HaloZone::opacity`] for how the two combine.
    pub color: String,
    /// X position as a fraction of the surface the halo is painted on:
    /// the viewport in a `slide` view, the world the camera travels in a
    /// `world` view (`WorldTimeline::world_extent`). 0.0 = left, 1.0 = right.
    #[serde(default = "default_half")]
    pub x: f32,
    /// Y position as a fraction of that same surface. 0.0 = top, 1.0 = bottom.
    #[serde(default = "default_half")]
    pub y: f32,
    /// Radius as a fraction of that surface's `max(width, height)` — so the
    /// same value covers proportionally the same area whichever view it is in.
    #[serde(default = "default_halo_radius")]
    pub radius: f32,
    /// Zone opacity, multiplied with any alpha already encoded in `color`.
    ///
    /// Default `1.0` is a true no-op: it leaves `color`'s own alpha (opaque
    /// or hex-encoded) untouched, so scenarios written before this field
    /// existed — including ones that hid alpha inside the hex string, e.g.
    /// `#1E3A8A55` — keep rendering identically. Values are clamped to
    /// `0.0..=1.0`.
    #[serde(default = "default_halo_opacity")]
    pub opacity: f32,
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

/// Constat #5: no derived `JsonSchema` here (the `#[serde(flatten)]` map
/// makes a fully-accurate derive impossible anyway — the point of `flatten`
/// is "any other keys"), which is exactly why `Scene`/`View` reached for
/// `#[schemars(skip)]` on `background` in the first place: skip was the
/// only option with no `JsonSchema` impl to call. But `Scene`/`View` are
/// also `deny_unknown_fields` (schemars emits `additionalProperties: false`
/// for that), so skipping `background` didn't just leave it undocumented —
/// it made the *exported schema* declare invalid any scenario that actually
/// sets `scene.background` / `view.background`, which is most of them. This
/// manual impl describes the real accepted shape (`$ref` + `transition` +
/// "anything else", matching the `flatten`) so `background` can be a real
/// declared property instead.
impl JsonSchema for BackgroundEntry {
    fn schema_name() -> String {
        "BackgroundEntry".to_string()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        use schemars::schema::*;

        let mut props = schemars::Map::new();
        props.insert("$ref".to_string(), gen.subschema_for::<Option<String>>());
        props.insert(
            "transition".to_string(),
            gen.subschema_for::<Option<BackgroundTransition>>(),
        );

        SchemaObject {
            instance_type: Some(InstanceType::Object.into()),
            object: Some(Box::new(ObjectValidation {
                properties: props,
                // Mirrors `#[serde(flatten)] overrides: serde_json::Map<..>`:
                // any other key (the preset config, `x`/`y`/`speed`/...) is
                // genuinely accepted, not a schema gap to close.
                additional_properties: Some(Box::new(Schema::Bool(true))),
                ..Default::default()
            })),
            ..Default::default()
        }
        .into()
    }
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

/// See [`BackgroundEntry`]'s `JsonSchema` impl doc comment — same reason:
/// `deserialize_background_value` is a hand-written `deserialize_with`, not
/// a derive, so there is no schema for schemars to infer without this.
impl JsonSchema for BackgroundValue {
    fn schema_name() -> String {
        "BackgroundValue".to_string()
    }

    fn json_schema(gen: &mut schemars::gen::SchemaGenerator) -> schemars::schema::Schema {
        use schemars::schema::*;

        let string_schema = gen.subschema_for::<String>();
        let entry_schema = gen.subschema_for::<BackgroundEntry>();
        let array_schema: Schema = SchemaObject {
            instance_type: Some(InstanceType::Array.into()),
            array: Some(Box::new(ArrayValidation {
                items: Some(SingleOrVec::Single(Box::new(entry_schema.clone()))),
                ..Default::default()
            })),
            ..Default::default()
        }
        .into();

        SchemaObject {
            subschemas: Some(Box::new(SubschemaValidation {
                one_of: Some(vec![string_schema, entry_schema, array_schema]),
                ..Default::default()
            })),
            ..Default::default()
        }
        .into()
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

fn default_halo_opacity() -> f32 {
    1.0
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

#[cfg(test)]
mod halo_zone_opacity_tests {
    use super::*;

    #[test]
    fn halo_zone_opacity_defaults_to_1_when_omitted() {
        let zone: HaloZone =
            serde_json::from_value(serde_json::json!({ "color": "#1E3A8A55" })).unwrap();
        assert_eq!(zone.opacity, 1.0);
    }

    #[test]
    fn halo_zone_opacity_respects_explicit_value() {
        let zone: HaloZone = serde_json::from_value(serde_json::json!({
            "color": "#1E3A8A",
            "opacity": 0.35
        }))
        .unwrap();
        assert_eq!(zone.opacity, 0.35);
    }

    #[test]
    fn halo_zone_serializes_opacity() {
        let zone = HaloZone {
            color: "#1E3A8A".to_string(),
            x: 0.5,
            y: 0.5,
            radius: 0.4,
            opacity: 0.6,
        };
        let v = serde_json::to_value(&zone).unwrap();
        // Compare as f64 with a tolerance: 0.6f32 widened to f64 is
        // 0.6000000238418579, not exactly 0.6 — an f32 precision artifact,
        // not a bug in the field itself.
        let got = v["opacity"]
            .as_f64()
            .expect("opacity must serialize as a number");
        assert!((got - 0.6).abs() < 1e-6, "got {got}");
    }

    #[test]
    fn animated_background_new_format_halo_zone_defaults_opacity() {
        // New nested format: {"preset":"halo","halo":{"zones":[...]}}
        let bg: AnimatedBackground = serde_json::from_value(serde_json::json!({
            "preset": "halo",
            "halo": { "zones": [{ "color": "#1E3A8A55", "x": 0.5, "y": 0.5, "radius": 0.4 }] },
            "speed": 0
        }))
        .unwrap();
        match bg.preset {
            BackgroundPreset::Halo(cfg) => {
                assert_eq!(cfg.zones.len(), 1);
                assert_eq!(cfg.zones[0].opacity, 1.0);
                // Alpha-in-hex is untouched by the schema layer — it stays in `color`.
                assert_eq!(cfg.zones[0].color, "#1E3A8A55");
            }
            _ => panic!("expected Halo preset"),
        }
    }

    #[test]
    fn animated_background_legacy_flat_format_halo_zone_defaults_opacity() {
        // Legacy flat format: {"preset":"halo","zones":[...]} with no sub-object.
        let bg: AnimatedBackground = serde_json::from_value(serde_json::json!({
            "preset": "halo",
            "zones": [{ "color": "#1E3A8A55", "x": 0.5, "y": 0.5, "radius": 0.4 }]
        }))
        .unwrap();
        match bg.preset {
            BackgroundPreset::Halo(cfg) => {
                assert_eq!(cfg.zones[0].opacity, 1.0);
            }
            _ => panic!("expected Halo preset"),
        }
    }
}

/// Constat #3: `AnimatedBackground::deserialize` had (at least) three silent
/// sinks — an unknown `preset` name silently became `gradient_shift` with
/// `colors: []`; a malformed/mistyped `zones` array in the legacy `halo`
/// form silently emptied via `.ok().unwrap_or_default()`; and a
/// malformed/missing `colors` (or `gradient_type`) on the legacy
/// `gradient_shift` form did the exact same `.ok().unwrap_or_default()`
/// silent-empty even when `preset` was spelled *correctly* — which is the
/// worst-case symptom named in the audit: an entirely black video with zero
/// diagnostics, because an empty-colors gradient paints black. Also found
/// (and fixed alongside, same root cause: the legacy branch's `_ =>`
/// wildcard): a *correctly spelled* `"heropattern"` preset written in the
/// legacy flat form (no `heropattern: {...}` sub-object) silently fell
/// through to `gradient_shift` too, because the legacy match only had
/// explicit arms for `grid_dots`/`concentric_circles`/`halo`.
#[cfg(test)]
mod animated_background_silent_sink_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn known_preset_gradient_shift_still_works() {
        let bg: AnimatedBackground = serde_json::from_value(json!({
            "preset": "gradient_shift",
            "colors": ["#111111", "#222222"],
            "gradient_type": "radial",
            "speed": 10
        }))
        .unwrap();
        match bg.preset {
            BackgroundPreset::GradientShift(cfg) => {
                assert_eq!(cfg.colors, vec!["#111111", "#222222"]);
                assert!(matches!(cfg.gradient_type, GradientType::Radial));
            }
            other => panic!("expected GradientShift, got {other:?}"),
        }
    }

    #[test]
    fn unknown_preset_name_is_a_named_error_not_a_silent_black_gradient() {
        let err = serde_json::from_value::<AnimatedBackground>(json!({
            "preset": "starfield",
            "speed": 10
        }))
        .expect_err("an unknown preset must be rejected, not silently treated as gradient_shift");
        let msg = err.to_string();
        assert!(
            msg.contains("starfield"),
            "error must name the offending preset value, got: {msg}"
        );
    }

    #[test]
    fn missing_preset_key_is_a_named_error() {
        let err = serde_json::from_value::<AnimatedBackground>(json!({ "speed": 10 }))
            .expect_err("a missing `preset` must be rejected, not silently treated as gradient_shift with colors: []");
        assert!(
            err.to_string().to_lowercase().contains("preset"),
            "got: {err}"
        );
    }

    #[test]
    fn legacy_halo_zones_still_work() {
        let bg: AnimatedBackground = serde_json::from_value(json!({
            "preset": "halo",
            "zones": [{ "color": "#1E3A8A", "x": 0.1, "y": 0.2, "radius": 0.3 }]
        }))
        .unwrap();
        match bg.preset {
            BackgroundPreset::Halo(cfg) => assert_eq!(cfg.zones.len(), 1),
            other => panic!("expected Halo, got {other:?}"),
        }
    }

    #[test]
    fn legacy_halo_malformed_zones_is_a_named_error_not_a_silent_empty_zones() {
        let err = serde_json::from_value::<AnimatedBackground>(json!({
            "preset": "halo",
            "zones": [{ "color": "#1E3A8A", "x": "not-a-number" }]
        }))
        .expect_err("a malformed zones entry must be rejected, not silently emptied");
        assert!(
            err.to_string().contains("zones") || err.to_string().contains("x"),
            "error should point at the offending field, got: {err}"
        );
    }

    #[test]
    fn legacy_halo_missing_zones_is_a_named_error_not_a_silent_empty_zones() {
        let err = serde_json::from_value::<AnimatedBackground>(json!({ "preset": "halo" }))
            .expect_err("missing zones must be rejected, not silently treated as an empty halo");
        assert!(err.to_string().contains("zones"), "got: {err}");
    }

    #[test]
    fn legacy_gradient_shift_missing_colors_is_a_named_error_not_a_silent_black_gradient() {
        // This is the exact worst-case symptom the audit names: preset is
        // spelled *correctly*, but colors is missing/malformed -> silently
        // empty colors -> a fully transparent gradient that paints black,
        // with no diagnostic at all.
        let err = serde_json::from_value::<AnimatedBackground>(json!({
            "preset": "gradient_shift",
            "speed": 5
        }))
        .expect_err("missing colors must error, not silently produce an empty (black) gradient");
        assert!(err.to_string().contains("colors"), "got: {err}");
    }

    #[test]
    fn legacy_heropattern_is_recognised_not_silently_turned_into_gradient_shift() {
        let bg: AnimatedBackground = serde_json::from_value(json!({
            "preset": "heropattern",
            "pattern": "plus",
            "color": "#ffffff",
            "opacity": 0.2,
            "scale": 1.5
        }))
        .unwrap();
        match bg.preset {
            BackgroundPreset::Heropattern(cfg) => {
                assert_eq!(cfg.pattern, "plus");
                assert_eq!(cfg.scale, 1.5);
            }
            other => panic!("expected Heropattern, got {other:?}"),
        }
    }

    #[test]
    fn legacy_heropattern_missing_pattern_is_a_named_error() {
        let err = serde_json::from_value::<AnimatedBackground>(json!({
            "preset": "heropattern"
        }))
        .expect_err("heropattern with no pattern name must error");
        assert!(err.to_string().contains("pattern"), "got: {err}");
    }

    #[test]
    fn direction_typo_is_a_named_error_not_a_silently_dropped_none() {
        let err = serde_json::from_value::<AnimatedBackground>(json!({
            "preset": "grid_dots",
            "colors": ["#fff"],
            "direction": "diagonal"
        }))
        .expect_err("an unrecognised direction must be rejected, not silently dropped to None");
        assert!(err.to_string().contains("direction"), "got: {err}");
    }

    #[test]
    fn direction_still_works_when_valid() {
        let bg: AnimatedBackground = serde_json::from_value(json!({
            "preset": "grid_dots",
            "colors": ["#fff"],
            "direction": "up"
        }))
        .unwrap();
        assert!(matches!(bg.direction, Some(ScrollDirection::Up)));
    }
}
