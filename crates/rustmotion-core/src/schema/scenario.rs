use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use super::animation::EasingType;
use super::background::{
    deserialize_animated_backgrounds, deserialize_background_value, AnimatedBackground,
    BackgroundValue, ResolvedBackground,
};
use super::shake::SceneShake;
use super::style::{CardAlign, CardDirection, CardJustify};
use super::time::TimePoint;

/// Definition of a variable in a structural component.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct VariableDefinition {
    #[serde(rename = "type")]
    pub var_type: VariableType,
    pub default: serde_json::Value,
    /// Optional description for documentation/schema.
    #[serde(default)]
    pub description: Option<String>,
}

/// Supported variable types.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum VariableType {
    String,
    Number,
    Boolean,
    Object,
    Array,
}

/// One entry of [`Scenario::components`] — a named, reusable template.
///
/// Schema-only twin of `rustmotion_core::expand::ComponentDefinition`
/// (private to that module — this type exists so `components` has a real,
/// documented shape in the exported schema; see [`Scenario::components`]'s
/// doc for why it can't just reuse that private struct, and why this one is
/// never populated in practice).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComponentTemplateDef {
    /// Parameters this template accepts. Same shape as
    /// [`ComponentTemplateParam`] — *not* [`VariableDefinition`] (the
    /// scenario-level `config` entry shape): a template parameter's
    /// `default` is itself optional, and omitting it is what makes the
    /// parameter required at every `use` site. `config`'s `default` has no
    /// such omission story — it is always required there.
    #[serde(default)]
    pub params: HashMap<String, ComponentTemplateParam>,
    /// The subtree to instantiate: a single component object, or an array
    /// of sibling component objects (a fragment spliced in place). May
    /// itself contain nested `for-each`/`use` directives.
    pub template: serde_json::Value,
}

/// One parameter declared by a [`ComponentTemplateDef`]. See
/// [`ComponentTemplateDef::params`] for how this differs from
/// [`VariableDefinition`].
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ComponentTemplateParam {
    #[serde(rename = "type")]
    pub param_type: VariableType,
    /// Omitting this makes the parameter required at every `use` site.
    #[serde(default)]
    pub default: Option<serde_json::Value>,
    #[serde(default)]
    pub description: Option<String>,
}

/// Deliberately **not** `#[derive(Deserialize)]` — see
/// [`ScenarioDe`]/`impl From<ScenarioDe> for Scenario` just below for why:
/// this type needs a post-deserialize pass (propagating `bpm`/`beat_offset`/
/// `timing`/`snap` down onto every reachable [`Scene`]) that a plain derive
/// can't express, and every call site that builds a `Scenario` from JSON
/// (`loader.rs`, `include.rs`'s own included-file loading) is outside this
/// workstream's owned files this wave — the propagation has to happen
/// *inside* `Scenario::deserialize` itself so those callers need no changes.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    #[serde(default = "default_version")]
    pub version: String,
    pub video: VideoConfig,
    /// File-based tracks (`"audio": [...]`, unchanged since before issue
    /// #331) or, for a scenario that synthesises its own soundtrack, a
    /// single object carrying both (`"audio": {"tracks": [...], "voices":
    /// {...}, "score": [...], "master": {...}}`) — see [`AudioValue`].
    #[serde(default)]
    pub audio: AudioValue,
    #[serde(default)]
    pub fonts: Vec<FontEntry>,
    #[serde(default, deserialize_with = "deserialize_scene_entries")]
    pub scenes: Vec<SceneEntry>,
    /// Composition: a sequence of views (slide or world). Mutually exclusive with top-level `scenes`.
    #[serde(default)]
    pub composition: Option<Vec<View>>,
    /// Config definitions for structural components. Each config entry has a type and default value.
    #[serde(default)]
    pub config: Option<HashMap<String, VariableDefinition>>,
    /// Named background templates that scenes can reference via `$ref`.
    #[serde(default)]
    pub backgrounds: HashMap<String, serde_json::Value>,
    /// Reusable component template definitions — `"components": { "name": {
    /// "params": {...}, "template": {...} } }`, instantiated from a
    /// `children` entry via `{"use": "name", "props": {...}}` (see
    /// [`ComponentTemplateDef`]). Documented in `CLAUDE.md`'s
    /// "Factorisation" section.
    ///
    /// This field exists so the exported JSON Schema (`rustmotion schema`)
    /// declares the shape the engine actually accepts — it is **not** how
    /// `components` is consumed at runtime. `rustmotion_core::expand::
    /// expand_directives` (a sibling workstream's file, outside this one's
    /// scope) resolves every `for-each`/`use` against this block and then
    /// *removes the key entirely*, before this struct is ever deserialized
    /// — the same way `variables::apply_variables` consumes `config`'s
    /// `$name` placeholders without `config` disappearing from the struct.
    /// The practical difference: `config` stays meaningful after expansion
    /// (`Scenario::config` is read elsewhere), while `components` is spent
    /// in full during expansion, so this field is always empty by the time
    /// any code outside `schema/` could read it.
    #[serde(default)]
    pub components: HashMap<String, ComponentTemplateDef>,
    /// Studio feedback annotations. Persisted in the scenario but never read by
    /// the renderer or the geometry validator. Skipped on serialization when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
    /// Beats per minute for this scenario's beat grid (issue #336). `None`
    /// (default) means no grid exists: any [`TimePoint`] beat (`b`) unit
    /// then fails to resolve with `unresolved_beat_unit` (schema:
    /// [`crate::schema::TimeError::NoBpm`]).
    #[serde(default)]
    pub bpm: Option<f64>,
    /// Where beat 0 sits on the scenario's absolute timeline, in seconds.
    /// The grid is `beat_offset + n * 60 / bpm` — not decoration: a reel
    /// whose first beat lands at 2.2s anchors its grid there, not at 0.
    #[serde(default)]
    pub beat_offset: f64,
    /// Optional override of the scenario's total rendered duration, in
    /// seconds. Reserved for downstream consumers (audio/export tooling);
    /// the frame-task scheduler in `rustmotion`'s `encode` module does not
    /// read it — under `timing: "v2"` the total is always `at_last +
    /// duration_last`, computed from the scenes themselves.
    #[serde(default)]
    pub duration: Option<f64>,
    /// Scene-placement semantics. See [`TimingMode`].
    #[serde(default)]
    pub timing: TimingMode,
    /// Rounds resolved times onto a grid before scheduling. See [`SnapMode`].
    #[serde(default)]
    pub snap: Option<SnapMode>,
    /// Scenario-level variables (issue #329): a scalar declared once,
    /// animated on the scenario's absolute timeline, and readable from any
    /// expression in any scene as `$name`. Visible everywhere; shadowed by
    /// a scene's own [`Scene::vars`] of the same name. See
    /// `rustmotion_core::vars` for the resolver and the `Scope`
    /// implementation that reads this.
    #[serde(default)]
    pub vars: crate::vars::VarSet,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ScenarioDe {
    #[serde(default = "default_version")]
    version: String,
    video: VideoConfig,
    #[serde(default)]
    audio: AudioValue,
    #[serde(default)]
    fonts: Vec<FontEntry>,
    #[serde(default, deserialize_with = "deserialize_scene_entries")]
    scenes: Vec<SceneEntry>,
    #[serde(default)]
    composition: Option<Vec<View>>,
    #[serde(default)]
    config: Option<HashMap<String, VariableDefinition>>,
    #[serde(default)]
    backgrounds: HashMap<String, serde_json::Value>,
    #[serde(default)]
    components: HashMap<String, ComponentTemplateDef>,
    #[serde(default)]
    annotations: Vec<Annotation>,
    #[serde(default)]
    bpm: Option<f64>,
    #[serde(default)]
    beat_offset: f64,
    #[serde(default)]
    duration: Option<f64>,
    #[serde(default)]
    timing: TimingMode,
    #[serde(default)]
    snap: Option<SnapMode>,
    #[serde(default)]
    vars: crate::vars::VarSet,
}

impl<'de> Deserialize<'de> for Scenario {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = ScenarioDe::deserialize(deserializer)?;
        let mut scenario = Scenario {
            version: raw.version,
            video: raw.video,
            audio: raw.audio,
            fonts: raw.fonts,
            scenes: raw.scenes,
            composition: raw.composition,
            config: raw.config,
            backgrounds: raw.backgrounds,
            components: raw.components,
            annotations: raw.annotations,
            bpm: raw.bpm,
            beat_offset: raw.beat_offset,
            duration: raw.duration,
            timing: raw.timing,
            snap: raw.snap,
            vars: raw.vars,
        };
        scenario.propagate_time_ctx();
        Ok(scenario)
    }
}

impl Scenario {
    fn propagate_time_ctx(&mut self) {
        let ctx = super::time::TimeCtx {
            bpm: self.bpm,
            beat_offset: self.beat_offset,
            scene_start: 0.0,
        };
        let timing = self.timing;
        let snap = self.snap;
        let scenario_vars = self.vars.clone();
        stamp_entries(&mut self.scenes, ctx, timing, snap, &scenario_vars);
        if let Some(views) = &mut self.composition {
            for view in views {
                stamp_entries(&mut view.scenes, ctx, timing, snap, &scenario_vars);
            }
        }
    }
}

fn stamp_entries(
    entries: &mut [SceneEntry],
    ctx: super::time::TimeCtx,
    timing: TimingMode,
    snap: Option<SnapMode>,
    scenario_vars: &crate::vars::VarSet,
) {
    for entry in entries {
        if let SceneEntry::Scene(scene) = entry {
            scene.resolved_time_ctx = ctx;
            scene.resolved_timing = timing;
            scene.resolved_snap = snap;
            scene.resolved_scenario_vars = scenario_vars.clone();
        }
    }
}

/// Scene-placement semantics for a scenario (issue #336). See
/// [`Scene::at`] and [`Scene::tail`] for what `"v2"` unlocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TimingMode {
    /// Today's behaviour, byte-identical for every scenario that predates
    /// issue #336: a transition's frames *replace* frames at the tail of
    /// the outgoing scene and the head of the incoming one, so the
    /// scenario's rendered length is `sum(scene durations) - sum(transition
    /// durations)`.
    #[default]
    V1,
    /// Absolute scene placement with a declared overlap: scene *i* occupies
    /// `[at_i, at_i + duration_i)`, and a transition entering it
    /// additionally renders the *previous* scene during `[at_i, at_i +
    /// transition duration)` — past its own end, per that scene's
    /// [`SceneTail`]. Total duration is `at_last + duration_last`: no
    /// subtraction anywhere.
    V2,
}

/// What `Scenario.snap` rounds resolved times onto. Currently only the beat
/// grid; the variant exists so the field reads as intent (`"beat"`) rather
/// than a bare boolean.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SnapMode {
    /// Round to the nearest point on `beat_offset + n * 60 / bpm`.
    Beat,
}

/// Lifecycle of a studio annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum AnnotationStatus {
    #[default]
    Open,
    Resolved,
}

/// What an annotation points at: a JSON Pointer into the source scenario,
/// plus optional context captured at click time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AnnotationTarget {
    /// RFC 6901 JSON Pointer into the source scenario (e.g. "/scenes/2/children/5").
    pub pointer: String,
    /// Component kind label captured at click time (e.g. "text", "card").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Bounding box [x, y, w, h] in video coords at the capture frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<[f32; 4]>,
}

/// A single studio feedback note attached to an element at a moment in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Annotation {
    /// Stable id (generated by the studio).
    pub id: String,
    /// Free-text change request for the agent/skill.
    pub note: String,
    #[serde(default)]
    pub status: AnnotationStatus,
    /// Frame index at capture time (global playhead).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<u32>,
    /// Resolved view index (convenience for the skill).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<usize>,
    /// Resolved scene index (convenience for the skill).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<usize>,
    pub target: AnnotationTarget,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ViewType {
    Slide,
    World,
}

fn default_view_type() -> ViewType {
    ViewType::Slide
}

fn default_camera_pan_duration() -> f64 {
    0.8
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct View {
    #[serde(rename = "type", default = "default_view_type")]
    pub view_type: ViewType,
    #[serde(default, deserialize_with = "deserialize_scene_entries")]
    pub scenes: Vec<SceneEntry>,
    /// Transition entering this view (between views).
    #[serde(default)]
    pub transition: Option<Transition>,
    /// (world) Shared background: color string, animated entry, or array.
    #[serde(default, deserialize_with = "deserialize_background_value")]
    pub background: Option<BackgroundValue>,
    /// (world) Legacy shared animated backgrounds.
    #[serde(
        default,
        rename = "animated-background",
        deserialize_with = "deserialize_animated_backgrounds"
    )]
    pub animated_background: Vec<AnimatedBackground>,
    /// (world) Easing for camera pan between scenes.
    #[serde(default = "default_transition_easing")]
    pub camera_easing: EasingType,
    /// (world) Duration of camera pan between scenes (default 0.8s).
    #[serde(default = "default_camera_pan_duration")]
    pub camera_pan_duration: f64,
}

#[derive(Debug)]
pub struct ResolvedScenario {
    pub video: VideoConfig,
    pub audio: Vec<AudioTrack>,
    pub fonts: Vec<FontEntry>,
    pub views: Vec<ResolvedView>,
    pub included_paths: Vec<std::path::PathBuf>,
}

impl ResolvedScenario {
    pub fn all_scenes(&self) -> impl Iterator<Item = &Scene> {
        self.views.iter().flat_map(|v| v.scenes.iter())
    }

    #[allow(dead_code)]
    pub fn all_scenes_vec(&self) -> Vec<&Scene> {
        self.all_scenes().collect()
    }
}

#[derive(Debug)]
pub struct ResolvedView {
    pub view_type: ViewType,
    pub scenes: Vec<Scene>,
    pub transition: Option<Transition>,
    pub background: ResolvedBackground,
    pub camera_easing: EasingType,
    pub camera_pan_duration: f64,
}

/// An entry in the `scenes` array: either a concrete scene or an include directive.
///
/// `#[serde(untagged)]` is kept so [`schemars`] still emits the correct
/// (flat, non-wrapped) JSON Schema for this enum, and so a direct
/// `SceneEntry::deserialize` call elsewhere keeps working. It is **not**
/// how `Scenario.scenes` / `View.scenes` actually deserialize an entry from
/// JSON, though: those two fields use [`deserialize_scene_entries`] instead
/// (see its doc comment for why — M6, issue #110).
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
#[allow(clippy::large_enum_variant)]
pub enum SceneEntry {
    /// A regular scene defined inline.
    Scene(Scene),
    /// A reference to an external scenario file whose scenes will be injected here.
    Include(IncludeDirective),
}

fn deserialize_scene_entries<'de, D>(deserializer: D) -> Result<Vec<SceneEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use serde::de::Error as _;

    let raw: Vec<serde_json::Value> = Vec::deserialize(deserializer)?;
    let mut out = Vec::with_capacity(raw.len());
    for (i, entry) in raw.into_iter().enumerate() {
        let is_include = entry.get("include").is_some();
        if is_include {
            let directive: IncludeDirective = serde_json::from_value(entry)
                .map_err(|e| D::Error::custom(format!("scenes[{i}] (include directive): {e}")))?;
            out.push(SceneEntry::Include(directive));
        } else {
            let scene: Scene = serde_json::from_value(entry)
                .map_err(|e| D::Error::custom(format!("scenes[{i}]: {e}")))?;
            out.push(SceneEntry::Scene(scene));
        }
    }
    Ok(out)
}

/// Directive to inject scenes from an external scenario file.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct IncludeDirective {
    /// Path (relative to parent file) or URL (http/https) to a scenario JSON file.
    pub include: String,
    /// Only include scenes at these 0-based indices. When absent, all scenes are included.
    #[serde(default)]
    pub scenes: Option<Vec<usize>>,
    /// Config overrides to pass to the included structural component.
    #[serde(default)]
    pub config: Option<HashMap<String, serde_json::Value>>,
}

/// Font file to load at startup.
///
/// Two mutually exclusive modes:
/// - **Local**: `path` (required) + `family`. Loads a `.ttf`/`.otf` file directly.
/// - **Google Fonts**: `source = "google"` + `family` + optional `weights` (default [400]).
///   The font is downloaded from the Google Fonts CSS2 API and cached in
///   `~/.cache/rustmotion/fonts` (or `%LOCALAPPDATA%\rustmotion\fonts` on Windows).
///   Subsequent renders with a warm cache make zero network calls.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FontEntry {
    /// Local file path (.ttf/.otf). Required when `source` is absent.
    #[serde(default)]
    pub path: Option<String>,
    /// Font family name (e.g. "Inter", "JetBrains Mono").
    pub family: String,
    /// Font source. Currently the only recognised value is `"google"`.
    /// When set, `path` must be absent.
    #[serde(default)]
    pub source: Option<String>,
    /// Font weights to download (Google Fonts only). Defaults to `[400]`.
    #[serde(default)]
    pub weights: Option<Vec<u16>>,
}

/// `Scenario::audio`'s wire shape (issue #331): either the legacy bare
/// array of file-based [`AudioTrack`]s, or a single object that can carry
/// both file tracks *and* a synthesised score, mixed into one bus. Which
/// shape a given `"audio"` value is is unambiguous from its JSON kind
/// (array vs. object), so `#[serde(untagged)]` needs no help distinguishing
/// them.
///
/// An old scenario using the bare-array form is unaffected byte-for-byte —
/// [`AudioValue::Tracks`] round-trips through exactly the type `audio` used
/// to be, and the `rustmotion` crate's include-resolution pass (outside
/// this crate) never has to look past [`AudioValue::into_tracks`].
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum AudioValue {
    Tracks(Vec<AudioTrack>),
    Config(Box<AudioConfig>),
}

impl Default for AudioValue {
    fn default() -> Self {
        AudioValue::Tracks(Vec::new())
    }
}

impl AudioValue {
    pub fn tracks(&self) -> &[AudioTrack] {
        match self {
            AudioValue::Tracks(tracks) => tracks,
            AudioValue::Config(config) => &config.tracks,
        }
    }

    pub fn into_tracks(self) -> Vec<AudioTrack> {
        match self {
            AudioValue::Tracks(tracks) => tracks,
            AudioValue::Config(config) => config.tracks,
        }
    }

    pub fn config(&self) -> Option<&AudioConfig> {
        match self {
            AudioValue::Tracks(_) => None,
            AudioValue::Config(config) => Some(config),
        }
    }
}

/// The object form of [`AudioValue`] — file tracks plus, optionally, a
/// synthesised score (issue #331's `voices`/`score`/`master`, matching the
/// issue body's example verbatim). `bpm`/`beat_offset` default to the
/// *scenario's* own [`Scenario::bpm`]/[`Scenario::beat_offset`] when
/// absent here — see `AudioConfig::as_score`'s caller in `rustmotion`'s
/// `encode` crate, which is where that fallback is actually applied (this
/// crate only carries the override, it does not resolve it: `Scenario`
/// alone does not know its own `bpm` by the time `include::resolve_includes`
/// has consumed it — the caller captures both before that happens).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioConfig {
    #[serde(default)]
    pub tracks: Vec<AudioTrack>,
    /// Overrides the scenario's own `bpm` for this score only. Normally
    /// left absent — sharing the scenario's real grid (deliverable #1's
    /// whole point) is the common case, not the exception.
    #[serde(default)]
    pub bpm: Option<f64>,
    #[serde(default)]
    pub beat_offset: Option<f64>,
    #[serde(default)]
    pub voices: HashMap<String, crate::audio::Voice>,
    #[serde(default)]
    pub score: Vec<crate::audio::ScoreEvent>,
    #[serde(default)]
    pub master: crate::audio::MasterBus,
}

impl AudioConfig {
    pub fn has_synth(&self) -> bool {
        !self.voices.is_empty() || !self.score.is_empty()
    }

    pub fn as_score(&self) -> crate::audio::Score {
        crate::audio::Score {
            voices: self.voices.clone(),
            score: self.score.clone(),
            master: self.master.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct WorldPosition {
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Scene {
    pub duration: f64,
    /// Unified background: color string, animated entry (with optional $ref), or array.
    #[serde(default, deserialize_with = "deserialize_background_value")]
    pub background: Option<BackgroundValue>,
    #[serde(default)]
    pub children: Vec<serde_json::Value>,
    #[serde(default)]
    pub transition: Option<Transition>,
    #[serde(default)]
    pub freeze_at: Option<f64>,
    /// Flex layout for automatic layer positioning
    #[serde(default)]
    pub layout: Option<SceneLayout>,
    /// Legacy animated background (kept for backward compat)
    #[serde(
        default,
        rename = "animated-background",
        deserialize_with = "deserialize_animated_backgrounds"
    )]
    pub animated_background: Vec<AnimatedBackground>,
    /// Virtual camera with animatable x, y, zoom, rotation.
    #[serde(default)]
    pub camera: Option<Camera>,
    /// Declarative camera shake (issue #330): a list of beat-synced
    /// impacts, each a damped harmonic oscillator, summed and **additive**
    /// over `camera` above — a pan and a shake ride the same frame instead
    /// of fighting over one keyframe track. See [`SceneShake`] for the
    /// formula and `crate::engine::shake::shake_offset` for the function
    /// that evaluates it.
    #[serde(default)]
    pub shake: Option<SceneShake>,
    /// Position of this scene in the 2D world (used by world views).
    #[serde(default, rename = "world-position")]
    pub world_position: Option<WorldPosition>,
    /// (world) Keep this scene visible after its time window ends.
    #[serde(default)]
    pub persist: bool,
    /// Post-processing effects applied to the full frame buffer after Skia renders.
    /// Effects are additive and applied in declaration order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effects: Vec<PostEffect>,
    /// Where this scene starts, on the scenario's absolute timeline (issue
    /// #336). `at` is absolute by definition — see the anchoring rule
    /// documented once on [`TimePoint`]. Defaults to [`SceneStart::Auto`]:
    /// immediately after the previous scene's own window ends. Only
    /// consulted by `rustmotion`'s frame-task scheduler under
    /// `Scenario.timing = "v2"`; a `"v1"` scenario places scenes
    /// back-to-back regardless of what this field says.
    #[serde(default, skip_serializing_if = "SceneStart::is_auto")]
    pub at: SceneStart,
    /// How this scene behaves when the *next* scene's transition overlaps
    /// into it, rendering it past its own `duration` (issue #336, `timing:
    /// "v2"` only). Defaults to [`SceneTail::Freeze`].
    #[serde(default, skip_serializing_if = "SceneTail::is_default_freeze")]
    pub tail: SceneTail,
    /// Post-resolution background (populated by include.rs, ignored by serde).
    #[serde(skip)]
    #[schemars(skip)]
    pub resolved_background: ResolvedBackground,
    /// The beat-grid context (`bpm`/`beat_offset`) of the `Scenario` this
    /// scene was declared in — populated by [`Scenario`]'s own
    /// `Deserialize` impl, *before* `include.rs` resolution ever runs, not
    /// by `include.rs` itself (issue #336; see the doc on
    /// [`ResolvedScenario`] for why). `scene_start` is always `0.0` here:
    /// [`Scene::at`] is resolved with
    /// [`TimePoint::resolve_absolute`](super::time::TimePoint::resolve_absolute),
    /// which ignores it.
    #[serde(skip)]
    #[schemars(skip)]
    pub resolved_time_ctx: super::time::TimeCtx,
    /// The `Scenario.timing` this scene was declared under. Same
    /// populate-at-deserialize-time note as [`Scene::resolved_time_ctx`].
    #[serde(skip)]
    #[schemars(skip)]
    pub resolved_timing: TimingMode,
    /// The `Scenario.snap` this scene was declared under. Same
    /// populate-at-deserialize-time note as [`Scene::resolved_time_ctx`].
    #[serde(skip)]
    #[schemars(skip)]
    pub resolved_snap: Option<SnapMode>,
    /// This scene's own declared variables (issue #329): shadow a
    /// same-named scenario-level variable ([`Scenario::vars`]) for
    /// expressions evaluated inside this scene, and are themselves
    /// invisible from any other scene — see `rustmotion_core::vars::VarScope`
    /// for the shadowing rule and why that isolation needs no special-case
    /// error handling. A variable with no `animation` here is a constant,
    /// same as at the scenario level.
    #[serde(default)]
    pub vars: crate::vars::VarSet,
    /// The enclosing [`Scenario::vars`] this scene was declared under —
    /// populated by `Scenario`'s own `Deserialize` impl, *before*
    /// `include.rs` resolution ever runs, not by `include.rs` itself. Same
    /// populate-at-deserialize-time note as [`Scene::resolved_time_ctx`]:
    /// [`ResolvedScenario`] carries no scenario-level fields of its own, so
    /// anything a scene needs to remember about its enclosing `Scenario`
    /// has to already be sitting on the `Scene` by the time `include.rs` —
    /// a file no single workstream owns — merges included scenes in.
    #[serde(skip)]
    #[schemars(skip)]
    pub resolved_scenario_vars: crate::vars::VarSet,
}

/// The literal string `"auto"` — the only value [`SceneStart::Auto`] can
/// hold. Its own tiny externally-tagged enum so it round-trips as the bare
/// string `"auto"` (serde's default representation for a fieldless
/// variant), matching every other `snake_case` string enum in this file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SceneStartAuto {
    Auto,
}

/// When a scene starts, on the scenario's absolute timeline. See
/// [`Scene::at`].
///
/// Not `#[derive(Deserialize)]` (see [`SceneStartDe`] / its manual
/// `Deserialize` impl just below): a syntactically malformed `at` — `"at":
/// "banana"` — must be rejected the moment the scenario is deserialized,
/// not discovered later as a silent fallback to automatic placement. A
/// derive can't express that extra grammar check, only the untagged
/// try-`"auto"`-then-`TimePoint` shape.
#[derive(Debug, Clone, PartialEq, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum SceneStart {
    /// Immediately after the previous scene's own window ends. Serializes
    /// and deserializes as the bare string `"auto"`.
    Auto(SceneStartAuto),
    /// An explicit point on the scenario's absolute timeline. Grammar is
    /// validated at deserialize time via
    /// [`TimePoint::validate_grammar`](super::time::TimePoint::validate_grammar)
    /// — resolving it (which additionally needs `bpm` for a beat unit) is a
    /// separate, later step: `rustmotion validate`'s schema pass, which can
    /// name the offending scene and report `unresolved_beat_unit` — see
    /// `crates/rustmotion/src/cli/commands/validate_schema.rs`.
    At(TimePoint),
}

#[derive(Deserialize)]
#[serde(untagged)]
enum SceneStartDe {
    Auto(SceneStartAuto),
    At(TimePoint),
}

impl<'de> Deserialize<'de> for SceneStart {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use serde::de::Error as _;
        match SceneStartDe::deserialize(deserializer)? {
            SceneStartDe::Auto(a) => Ok(SceneStart::Auto(a)),
            SceneStartDe::At(tp) => {
                tp.validate_grammar().map_err(D::Error::custom)?;
                Ok(SceneStart::At(tp))
            }
        }
    }
}

impl Default for SceneStart {
    fn default() -> Self {
        SceneStart::Auto(SceneStartAuto::Auto)
    }
}

impl SceneStart {
    fn is_auto(&self) -> bool {
        matches!(self, SceneStart::Auto(_))
    }
}

/// How a scene behaves when it is rendered past its own `duration` because
/// the *next* scene's transition overlaps into it. See [`Scene::tail`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum SceneTail {
    /// Hold the scene's last frame for the overlap — the same
    /// fixed-non-advancing-time effect `freeze_at` produces.
    #[default]
    Freeze,
    /// Let the scene's own animations keep running past its declared
    /// `duration` for the overlap.
    Continue,
}

impl SceneTail {
    fn is_default_freeze(&self) -> bool {
        matches!(self, SceneTail::Freeze)
    }
}

/// Virtual camera for pan/zoom/rotation effects at the scene level.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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
    /// Rotation in degrees around the camera origin. Default: 0.
    #[serde(default)]
    pub rotation: f32,
    /// Focal point for zoom/rotation, in frame pixels. Absent = frame centre
    /// (the historical behaviour). When the object is present, `x`/`y`
    /// default to 0 (top-left corner) — set both explicitly.
    #[serde(default)]
    pub origin: Option<CameraOrigin>,
    /// Keyframe animations for camera properties.
    #[serde(default)]
    pub keyframes: Vec<CameraKeyframe>,
}

/// Focal point of the camera in frame pixels.
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CameraOrigin {
    #[serde(default)]
    pub x: f32,
    #[serde(default)]
    pub y: f32,
}

const KNOWN_CAMERA_PROPERTIES: &[&str] = &["x", "y", "zoom", "rotation", "origin.x", "origin.y"];

fn validate_camera_property<E: serde::de::Error>(value: &str) -> Result<(), E> {
    if KNOWN_CAMERA_PROPERTIES.contains(&value) {
        return Ok(());
    }
    let normalize = |s: &str| s.replace(['-', '_', ' '], ".").to_lowercase();
    let normalized = normalize(value);
    if let Some(suggestion) = KNOWN_CAMERA_PROPERTIES
        .iter()
        .find(|known| normalize(known) == normalized)
    {
        Err(E::custom(format!(
            "unknown camera keyframe property '{value}' — did you mean '{suggestion}'?"
        )))
    } else {
        Err(E::custom(format!(
            "unknown camera keyframe property '{value}': expected one of {}",
            KNOWN_CAMERA_PROPERTIES.join(", ")
        )))
    }
}

fn deserialize_camera_property<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    validate_camera_property::<D::Error>(&s)?;
    Ok(s)
}

/// A keyframe for a camera property.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CameraKeyframe {
    /// The camera property to animate: "x", "y", "zoom", "rotation",
    /// "origin.x", "origin.y" (dotted form, matching the component keyframe
    /// convention for compound properties).
    #[serde(deserialize_with = "deserialize_camera_property")]
    pub property: String,
    /// Time-value pairs for the animation.
    pub values: Vec<CameraKeyframePoint>,
    /// Easing function for interpolation.
    #[serde(default)]
    pub easing: EasingType,
}

/// A single time-value point in a camera keyframe.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CameraKeyframePoint {
    /// Time in seconds (relative to scene start).
    pub time: f64,
    /// Value at this time.
    pub value: f32,
}

fn default_camera_zoom() -> f32 {
    1.0
}

/// Direction for progressive blur.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum BlurDirection {
    Top,
    #[default]
    Bottom,
}

/// A post-processing effect applied to the full frame buffer after Skia renders.
/// Effects are pure Rust, deterministic, and applied in declaration order.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PostEffect {
    /// Film grain noise overlaid on every pixel.
    Grain {
        /// Noise strength clamped to 0..1. Default: 0.15.
        #[serde(default = "default_grain_intensity")]
        intensity: f32,
        /// Base seed for the noise hash. Default: 42.
        #[serde(default = "default_grain_seed")]
        seed: u64,
        /// When true, the pattern changes per frame (`seed ^ frame_index`). Default: true.
        #[serde(default = "default_true")]
        animated: bool,
    },
    /// Darken the frame edges towards the corners.
    Vignette {
        /// Darkness strength clamped to 0..1. Default: 0.5.
        #[serde(default = "default_vignette_intensity")]
        intensity: f32,
        /// Fraction of the half-diagonal where darkening starts. Default: 0.75.
        #[serde(default = "default_vignette_radius")]
        radius: f32,
    },
    /// Reduce spatial resolution by averaging square pixel blocks.
    Pixelate {
        /// Block size in pixels, clamped to 1..=256. Default: 8.
        #[serde(default = "default_pixelate_size")]
        size: u32,
    },
    /// Blur that grows from zero at `start` to `max_radius` at the frame edge.
    ProgressiveBlur {
        /// Which edge gets the maximum blur. Default: bottom.
        #[serde(default)]
        direction: BlurDirection,
        /// Fraction of the frame height where blur begins (0.0..1.0). Default: 0.5.
        #[serde(default = "default_blur_start")]
        start: f32,
        /// Maximum box-blur radius in pixels at the far edge. Default: 12.0.
        #[serde(default = "default_blur_max_radius")]
        max_radius: f32,
    },
    /// A full-frame flash that decays from `intensity` to zero over
    /// `duration`, starting at `at` — issue #330's companion to
    /// [`super::shake::SceneShake`]: the reference reel this pair was
    /// built for lights one of these on every shake impact, so a beat-grid
    /// `at` can share the same value the matching
    /// [`super::shake::ShakeImpact::at`] uses. `at` is a [`TimePoint`],
    /// resolved the same way as any other in-scene time (relative to the
    /// scene's own start unless `@`-prefixed).
    Flash {
        /// When the flash starts, on the scene's own timeline.
        at: TimePoint,
        /// Flash colour as a hex string. Default: `"#FFFFFF"`.
        #[serde(default = "default_flash_color")]
        color: String,
        /// Peak strength, clamped to 0..1, at `t = at`. Default: 0.6.
        #[serde(default = "default_flash_intensity")]
        intensity: f32,
        /// How long the flash takes to decay to zero, in seconds. Default: 0.15.
        #[serde(default = "default_flash_duration")]
        duration: f32,
    },
}

fn default_grain_intensity() -> f32 {
    0.15
}
fn default_grain_seed() -> u64 {
    42
}
fn default_true() -> bool {
    true
}
fn default_vignette_intensity() -> f32 {
    0.5
}
fn default_vignette_radius() -> f32 {
    0.75
}
fn default_pixelate_size() -> u32 {
    8
}
fn default_blur_start() -> f32 {
    0.5
}
fn default_blur_max_radius() -> f32 {
    12.0
}
fn default_flash_color() -> String {
    "#FFFFFF".to_string()
}
fn default_flash_intensity() -> f32 {
    0.6
}
fn default_flash_duration() -> f32 {
    0.15
}

/// Scene-level flex layout configuration
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
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

/// The order `pixel_dissolve` turns its cells in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PixelDissolveOrder {
    /// From the frame's border inward, so the centre — where the subject
    /// usually is — is the last thing to go. This is what the reference piece
    /// does, and it is the default for that reason.
    #[default]
    EdgesIn,
    /// The mirror: the centre opens first and the border closes last.
    CenterOut,
    /// No spatial order at all — every cell on its own draw.
    Random,
}

/// The corner a `corner_reveal` is anchored to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransitionCorner {
    /// Measured default: the reference piece grows its reveal from here, with
    /// the right and top edges pinned and the left and bottom edges travelling.
    #[default]
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
}

/// Which way a directional transition travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TransitionDirection {
    /// Both frames travel leftwards; the incoming one enters from the right.
    #[default]
    Left,
    Right,
    Up,
    Down,
}

/// The centre a `zoom_blur` transition radiates its streaks from, in frame
/// pixels. Absent = frame centre.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct ZoomBlurOrigin {
    /// Horizontal centre, in frame pixels.
    #[serde(default)]
    pub x: f32,
    /// Vertical centre, in frame pixels.
    #[serde(default)]
    pub y: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Transition {
    #[serde(rename = "type")]
    pub transition_type: TransitionType,
    /// Which corner a `corner_reveal` grows from. Ignored by every other type.
    #[serde(default)]
    pub corner: TransitionCorner,
    /// Cell edge in px for `pixel_dissolve`. Ignored by every other type.
    #[serde(default = "default_transition_cell")]
    pub cell: f32,
    /// `pixel_dissolve` only: stable scatter selector. Two transitions with the
    /// same seed dissolve in the same order.
    #[serde(default = "default_transition_seed")]
    pub seed: u32,
    /// `pixel_dissolve` only: which cells turn first.
    #[serde(default)]
    pub order: PixelDissolveOrder,
    /// Which way a `chromatic_wipe` travels. Ignored by every other type —
    /// the `wipe_*`/`slide` family encodes its direction in the type name.
    #[serde(default)]
    pub direction: TransitionDirection,
    /// `chromatic_wipe` only: how far the red and cyan channels split at the
    /// peak of the wipe, as a multiple of the tuned default. `0` removes the
    /// colour flash and leaves a plain fast slide; `2` doubles it.
    #[serde(default = "default_transition_aberration")]
    pub aberration: f32,
    /// `zoom_blur` only: how far the radial streaks reach. `0` collapses the
    /// streak pass entirely, leaving a plain zoom with no smear; higher
    /// values pull the outer copies further from `origin`. Ignored by every
    /// other transition type.
    #[serde(default = "default_transition_strength")]
    pub strength: f32,
    /// `zoom_blur` only: the centre the streaks radiate from. Ignored by
    /// every other transition type.
    #[serde(default)]
    pub origin: Option<ZoomBlurOrigin>,
    #[serde(default = "default_transition_duration")]
    pub duration: f64,
    #[serde(default = "default_transition_easing")]
    pub easing: EasingType,
    /// How the background behaves during a `camera_pan`. Ignored by every
    /// other transition type, which composite two finished frames and have no
    /// separate background layer to move.
    #[serde(default)]
    pub background: PanBackground,
}

/// Whether a `camera_pan` treats the background as a fixed backdrop the scenes
/// slide across, or as part of each scene, travelling with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum PanBackground {
    /// The outgoing scene's background stays put while both foregrounds slide
    /// over it. Keeps a shared ambience continuous, so the cut is invisible —
    /// this is the default because it is what makes a multi-beat video read as
    /// one shot.
    #[default]
    Static,
    /// Each scene carries its own background, and both travel with their
    /// foreground. Use this when the beats are meant to look like different
    /// places rather than one continuous space.
    Travel,
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
    CornerReveal,
    PixelDissolve,
    CameraPan,
    /// A fast slide in which the reveal edge splits into its red and cyan
    /// channels at the peak and recombines as it lands — the glitch-flash
    /// cut. `direction` steers it, `aberration` scales the split.
    ChromaticWipe,
    /// A radial zoom blur: the outgoing frame streaks outward from `origin`
    /// while it fades, then the incoming frame is left standing alone — the
    /// "tunnel" cut. `strength` sets how far the streaks reach; `0`
    /// collapses it to a plain zoom with no smear. Zero at both ends of the
    /// transition, so no fringe bleeds into the next scene.
    ZoomBlur,
    None,
}

fn default_transition_cell() -> f32 {
    48.0
}

fn default_transition_seed() -> u32 {
    11
}

fn default_transition_aberration() -> f32 {
    1.0
}

fn default_transition_strength() -> f32 {
    1.0
}

fn default_transition_duration() -> f64 {
    0.5
}

pub(crate) fn default_transition_easing() -> EasingType {
    EasingType::EaseInOut
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum VideoCodec {
    #[default]
    H264,
    H265,
    Vp9,
    Prores,
}

fn default_version() -> String {
    "1.0".to_string()
}

fn default_fps() -> u32 {
    30
}

fn default_background() -> String {
    "#000000".to_string()
}

#[cfg(test)]
mod annotation_tests {
    use super::*;

    const MINIMAL: &str = r#"{ "video": { "width": 1920, "height": 1080 }, "scenes": [] }"#;

    const WITH_ANNOTATIONS: &str = r#"{
        "video": { "width": 1920, "height": 1080 },
        "scenes": [],
        "annotations": [
            {
                "id": "an_4f2a",
                "note": "reduce font-size",
                "status": "open",
                "frame": 142,
                "view": 0,
                "scene": 2,
                "target": {
                    "pointer": "/scenes/2/children/5",
                    "kind": "text",
                    "rect": [10.0, 20.0, 30.0, 40.0]
                }
            }
        ]
    }"#;

    #[test]
    fn scenario_without_annotations_defaults_empty() {
        let s: Scenario = serde_json::from_str(MINIMAL).unwrap();
        assert!(s.annotations.is_empty());
    }

    #[test]
    fn scenario_with_annotations_deserializes() {
        let s: Scenario = serde_json::from_str(WITH_ANNOTATIONS).unwrap();
        assert_eq!(s.annotations.len(), 1);
        let a = &s.annotations[0];
        assert_eq!(a.id, "an_4f2a");
        assert_eq!(a.status, AnnotationStatus::Open);
        assert_eq!(a.frame, Some(142));
        assert_eq!(a.view, Some(0));
        assert_eq!(a.scene, Some(2));
        assert_eq!(a.target.pointer, "/scenes/2/children/5");
        assert_eq!(a.target.kind.as_deref(), Some("text"));
        assert_eq!(a.target.rect, Some([10.0, 20.0, 30.0, 40.0]));
    }

    #[test]
    fn empty_annotations_are_not_serialized() {
        let s: Scenario = serde_json::from_str(MINIMAL).unwrap();
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            !json.contains("annotations"),
            "empty annotations must be skipped, got: {json}"
        );
    }

    #[test]
    fn status_defaults_to_open_and_target_fields_optional() {
        let json = r#"{
            "video": { "width": 1, "height": 1 },
            "scenes": [],
            "annotations": [ { "id": "x", "note": "n", "target": { "pointer": "/scenes/0" } } ]
        }"#;
        let s: Scenario = serde_json::from_str(json).unwrap();
        assert_eq!(s.annotations[0].status, AnnotationStatus::Open);
        assert_eq!(s.annotations[0].target.kind, None);
        assert_eq!(s.annotations[0].target.rect, None);
        assert_eq!(s.annotations[0].frame, None);
    }
}

#[cfg(test)]
mod scene_entry_error_tests {
    use super::*;

    #[test]
    fn missing_duration_names_the_scene_index_and_field_not_the_untagged_message() {
        let json = r#"{
            "video": { "width": 100, "height": 100 },
            "scenes": [
                { "duration": 1.0, "children": [] },
                { "children": [] }
            ]
        }"#;
        let err = serde_json::from_str::<Scenario>(json).expect_err("missing duration must fail");
        let msg = err.to_string();
        assert!(
            !msg.contains("did not match any variant of untagged enum"),
            "must not regress to the opaque untagged message: {msg}"
        );
        assert!(
            msg.contains("scenes[1]"),
            "must name the offending scene index: {msg}"
        );
        assert!(
            msg.contains("duration"),
            "must name the missing field: {msg}"
        );
    }

    #[test]
    fn misspelled_transition_type_names_itself() {
        let json = r#"{
            "video": { "width": 100, "height": 100 },
            "scenes": [
                {
                    "duration": 1.0,
                    "children": [],
                    "transition": { "type": "wip_left" }
                }
            ]
        }"#;
        let err =
            serde_json::from_str::<Scenario>(json).expect_err("bad transition type must fail");
        let msg = err.to_string();
        assert!(
            !msg.contains("did not match any variant of untagged enum"),
            "must not regress to the opaque untagged message: {msg}"
        );
        assert!(
            msg.contains("scenes[0]"),
            "must name the offending scene index: {msg}"
        );
        assert!(
            msg.contains("wip_left"),
            "must echo the bad value so the author can spot the typo: {msg}"
        );
    }

    #[test]
    fn include_directive_still_works() {
        let json = r#"{
            "video": { "width": 100, "height": 100 },
            "scenes": [
                { "include": "does/not/matter.json" }
            ]
        }"#;
        let s: Scenario = serde_json::from_str(json).expect("include entry must parse");
        assert!(matches!(s.scenes[0], SceneEntry::Include(_)));
    }

    #[test]
    fn broken_include_directive_names_itself() {
        let json = r#"{
            "video": { "width": 100, "height": 100 },
            "scenes": [
                { "include": "x.json", "scenes": "not-an-array" }
            ]
        }"#;
        let err = serde_json::from_str::<Scenario>(json).expect_err("must fail");
        let msg = err.to_string();
        assert!(msg.contains("scenes[0]"), "got: {msg}");
        assert!(msg.contains("include directive"), "got: {msg}");
    }

    #[test]
    fn view_scenes_field_uses_the_same_precise_errors() {
        let json = r#"{
            "video": { "width": 100, "height": 100 },
            "composition": [
                { "type": "slide", "scenes": [ { "children": [] } ] }
            ]
        }"#;
        let err = serde_json::from_str::<Scenario>(json).expect_err("must fail");
        let msg = err.to_string();
        assert!(
            !msg.contains("did not match any variant of untagged enum"),
            "got: {msg}"
        );
        assert!(msg.contains("scenes[0]"), "got: {msg}");
        assert!(msg.contains("duration"), "got: {msg}");
    }
}

#[cfg(test)]
mod strict_schema_tests {
    use super::*;

    #[test]
    fn misspelled_scene_field_is_rejected() {
        let json = r#"{
            "video": { "width": 100, "height": 100 },
            "scenes": [ { "durration": 3.0, "children": [] } ]
        }"#;
        let err = serde_json::from_str::<Scenario>(json).expect_err("must fail");
        assert!(err.to_string().contains("durration"), "got: {err}");
    }

    #[test]
    fn misspelled_video_field_is_rejected() {
        let json = r#"{
            "video": { "width": 100, "height": 100, "framerate": 30 },
            "scenes": [ { "duration": 1.0, "children": [] } ]
        }"#;
        let err = serde_json::from_str::<Scenario>(json).expect_err("must fail");
        assert!(err.to_string().contains("framerate"), "got: {err}");
    }

    #[test]
    fn misspelled_top_level_scenario_field_is_rejected() {
        let json = r#"{
            "video": { "width": 100, "height": 100 },
            "scenes": [ { "duration": 1.0, "children": [] } ],
            "titel": "typo"
        }"#;
        let err = serde_json::from_str::<Scenario>(json).expect_err("must fail");
        assert!(err.to_string().contains("titel"), "got: {err}");
    }

    #[test]
    fn misspelled_camera_field_is_rejected() {
        let json = r#"{
            "video": { "width": 100, "height": 100 },
            "scenes": [ {
                "duration": 1.0,
                "children": [],
                "camera": { "zooom": 1.5 }
            } ]
        }"#;
        let err = serde_json::from_str::<Scenario>(json).expect_err("must fail");
        assert!(err.to_string().contains("zooom"), "got: {err}");
    }

    #[test]
    fn misspelled_scene_layout_field_is_rejected() {
        let json = r#"{
            "video": { "width": 100, "height": 100 },
            "scenes": [ {
                "duration": 1.0,
                "children": [],
                "layout": { "gapp": 10 }
            } ]
        }"#;
        let err = serde_json::from_str::<Scenario>(json).expect_err("must fail");
        assert!(err.to_string().contains("gapp"), "got: {err}");
    }

    #[test]
    fn misspelled_view_field_is_rejected() {
        let json = r#"{
            "video": { "width": 100, "height": 100 },
            "composition": [ { "typ": "slide", "scenes": [] } ]
        }"#;
        let err = serde_json::from_str::<Scenario>(json).expect_err("must fail");
        assert!(err.to_string().contains("typ"), "got: {err}");
    }

    #[test]
    fn valid_scenario_with_every_covered_struct_still_parses() {
        let json = r##"{
            "version": "1.0",
            "video": { "width": 100, "height": 100, "fps": 30, "background": "#000000" },
            "scenes": [ {
                "duration": 1.0,
                "children": [],
                "layout": { "direction": "column", "gap": 10, "align_items": "center", "justify_content": "center", "padding": 5 },
                "transition": { "type": "fade", "duration": 0.5, "easing": "ease_in_out" },
                "camera": { "x": 0, "y": 0, "zoom": 1.0, "rotation": 0, "origin": { "x": 1, "y": 2 }, "keyframes": [ { "property": "zoom", "values": [ { "time": 0.0, "value": 1.0 } ], "easing": "linear" } ] }
            } ]
        }"##;
        let s: Scenario = serde_json::from_str(json).expect("valid scenario must still parse");
        assert_eq!(s.scenes.len(), 1);
    }
}

#[cfg(test)]
mod camera_keyframe_property_tests {
    use super::*;

    #[test]
    fn known_camera_properties_still_work() {
        for prop in ["x", "y", "zoom", "rotation", "origin.x", "origin.y"] {
            let json = format!(
                r#"{{ "property": "{prop}", "values": [ {{ "time": 0.0, "value": 1.0 }} ] }}"#
            );
            let kf: CameraKeyframe = serde_json::from_str(&json)
                .unwrap_or_else(|e| panic!("property '{prop}' must be accepted, got: {e}"));
            assert_eq!(kf.property, prop);
        }
    }

    #[test]
    fn unknown_camera_property_is_a_named_error_not_a_silent_no_op() {
        let json = r#"{ "property": "tilt", "values": [ { "time": 0.0, "value": 1.0 } ] }"#;
        let err = serde_json::from_str::<CameraKeyframe>(json).expect_err(
            "an unrecognised camera keyframe property must be rejected, not silently inert",
        );
        assert!(err.to_string().contains("tilt"), "got: {err}");
    }

    #[test]
    fn misspelled_origin_property_is_a_named_error() {
        let json = r#"{ "property": "origin_x", "values": [ { "time": 0.0, "value": 1.0 } ] }"#;
        let err = serde_json::from_str::<CameraKeyframe>(json)
            .expect_err("origin_x must be rejected — the real property is origin.x");
        let msg = err.to_string();
        assert!(msg.contains("origin_x"), "got: {msg}");
        assert!(
            msg.contains("origin.x"),
            "expected a did-you-mean nudge toward origin.x, got: {msg}"
        );
    }
}

#[cfg(test)]
mod scene_at_grammar_tests {
    use super::*;

    fn scenario_with_at(at: &str) -> String {
        format!(
            r#"{{
                "video": {{ "width": 100, "height": 100 }},
                "scenes": [
                    {{ "duration": 1.0, "children": [] }},
                    {{ "duration": 1.0, "children": [], "at": {at} }}
                ]
            }}"#
        )
    }

    #[test]
    fn syntactically_malformed_at_is_rejected_at_deserialize_time() {
        let json = scenario_with_at(r#""banana""#);
        let err = serde_json::from_str::<Scenario>(&json)
            .expect_err("a malformed `at` must fail to deserialize, not fall back silently");
        let msg = err.to_string();
        assert!(
            msg.contains("cannot parse time"),
            "expected the grammar error to surface, got: {msg}"
        );
        assert!(msg.contains("banana"), "got: {msg}");
    }

    #[test]
    fn grammatically_valid_beat_unit_deserializes_even_with_no_bpm() {
        let json = scenario_with_at(r#""@8b""#);
        let scenario: Scenario =
            serde_json::from_str(&json).expect("grammar-valid `at` must deserialize");
        let SceneEntry::Scene(ref scene) = scenario.scenes[1] else {
            panic!("expected a Scene entry");
        };
        assert!(matches!(scene.at, SceneStart::At(TimePoint::Spec(ref s)) if s == "@8b"));
    }

    #[test]
    fn auto_and_plain_seconds_still_deserialize() {
        let json = scenario_with_at(r#""auto""#);
        let scenario: Scenario = serde_json::from_str(&json).expect("auto must deserialize");
        let SceneEntry::Scene(ref scene) = scenario.scenes[1] else {
            panic!("expected a Scene entry");
        };
        assert!(matches!(scene.at, SceneStart::Auto(_)));

        let json = scenario_with_at("2.5");
        let scenario: Scenario = serde_json::from_str(&json).expect("bare number must deserialize");
        let SceneEntry::Scene(ref scene) = scenario.scenes[1] else {
            panic!("expected a Scene entry");
        };
        assert!(matches!(scene.at, SceneStart::At(TimePoint::Seconds(s)) if s == 2.5));
    }
}

#[cfg(test)]
mod scene_shake_and_flash_tests {
    use super::*;

    #[test]
    fn scene_shake_is_absent_by_default() {
        let json = r#"{ "duration": 1.0, "children": [] }"#;
        let scene: Scene = serde_json::from_str(json).unwrap();
        assert!(scene.shake.is_none());
    }

    #[test]
    fn scene_shake_deserializes_with_six_impacts_on_the_beat_grid() {
        let json = r##"{
            "duration": 4.0,
            "children": [],
            "shake": {
                "impacts": [
                    { "at": "0b", "amplitude": 24.0 },
                    { "at": "4b", "amplitude": 20.0 },
                    { "at": "8b", "amplitude": 20.0 },
                    { "at": "12b", "amplitude": 16.0 },
                    { "at": "16b", "amplitude": 16.0 },
                    { "at": "20b", "amplitude": 12.0 }
                ],
                "decay": 12.0,
                "frequency": 22.0,
                "rotation": 0.3
            }
        }"##;
        let scene: Scene = serde_json::from_str(json).unwrap();
        let shake = scene.shake.expect("shake must deserialize");
        assert_eq!(shake.impacts.len(), 6);
        assert_eq!(shake.decay, 12.0);
        assert_eq!(shake.frequency, 22.0);
        assert_eq!(shake.rotation, 0.3);
    }

    #[test]
    fn scene_shake_and_camera_coexist_on_the_same_scene() {
        let json = r#"{
            "duration": 2.0,
            "children": [],
            "camera": { "zoom": 1.2 },
            "shake": { "impacts": [ { "at": 0.5, "amplitude": 10.0 } ] }
        }"#;
        let scene: Scene = serde_json::from_str(json).unwrap();
        assert!(scene.camera.is_some());
        assert!(scene.shake.is_some());
    }

    #[test]
    fn misspelled_shake_field_is_rejected() {
        let json = r#"{
            "video": { "width": 100, "height": 100 },
            "scenes": [ {
                "duration": 1.0,
                "children": [],
                "shake": { "impacts": [], "decayy": 5.0 }
            } ]
        }"#;
        let err = serde_json::from_str::<Scenario>(json).expect_err("must fail");
        assert!(err.to_string().contains("decayy"), "got: {err}");
    }

    #[test]
    fn post_effect_flash_deserializes_and_defaults() {
        let json = r#"{ "type": "flash", "at": "4b" }"#;
        let effect: PostEffect = serde_json::from_str(json).unwrap();
        match effect {
            PostEffect::Flash {
                at,
                color,
                intensity,
                duration,
            } => {
                assert_eq!(at, TimePoint::Spec("4b".to_string()));
                assert_eq!(color, "#FFFFFF");
                assert_eq!(intensity, 0.6);
                assert_eq!(duration, 0.15);
            }
            other => panic!("expected Flash, got {other:?}"),
        }
    }

    #[test]
    fn post_effect_flash_accepts_every_known_field() {
        let json = r##"{
            "type": "flash",
            "at": 1.2,
            "color": "#FF3300",
            "intensity": 0.9,
            "duration": 0.08
        }"##;
        let effect: PostEffect = serde_json::from_str(json).unwrap();
        match effect {
            PostEffect::Flash {
                at,
                color,
                intensity,
                duration,
            } => {
                assert_eq!(at, TimePoint::Seconds(1.2));
                assert_eq!(color, "#FF3300");
                assert_eq!(intensity, 0.9);
                assert_eq!(duration, 0.08);
            }
            other => panic!("expected Flash, got {other:?}"),
        }
    }

    #[test]
    fn scene_effects_accept_a_flash_alongside_other_post_effects() {
        let json = r#"{
            "duration": 1.0,
            "children": [],
            "effects": [
                { "type": "flash", "at": "0b" },
                { "type": "grain", "intensity": 0.1 }
            ]
        }"#;
        let scene: Scene = serde_json::from_str(json).unwrap();
        assert_eq!(scene.effects.len(), 2);
        assert!(matches!(scene.effects[0], PostEffect::Flash { .. }));
        assert!(matches!(scene.effects[1], PostEffect::Grain { .. }));
    }
}
