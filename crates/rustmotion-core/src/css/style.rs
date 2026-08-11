//! `CssStyle` — typed mirror of the CSS properties supported by the engine.
//!
//! Scope: Remotion-equivalent (Flex, Grid, Block, transforms 2D/3D, filters,
//! gradients, position absolute/relative, box-shadow, border-radius, opacity,
//! clip-path). Excludes: inline boxes, floats, tables, position sticky/fixed.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::units::{Length, LengthContext, LengthPercentage, ParsedLength};
// `GradientBorder` / `InnerShadow` are reused from the schema layer rather
// than mirrored: same crate, same serde/JsonSchema derives, identical JSON
// shape either way — a css-local mirror would only duplicate the struct.
use crate::schema::{deserialize_animation_effects, AnimationEffect, GradientBorder, InnerShadow};

// ─── Legibility floor (relocated from `rustmotion-cli/src/commands/
// geometry.rs`'s `check_legibility`, issue #110/#102 — moved here, not
// duplicated, so `text-autofit` below can shrink down to the exact same
// calibrated threshold instead of inventing a second one; `rustmotion-cli`
// depends on `rustmotion-core`, never the other way around, so the shared
// value has to live on this side of that boundary) ─────────────────────────
//
// Threshold justification (rendered evidence, not a guess): a 1920×1080
// scenario was rendered with the same sample line at 8/10/11/12/13/14/16/18/
// 20/22/24/28px, then the frame was scaled down 50% (a realistic "not
// full-native" viewing size) to inspect. 8–13px degraded to an illegible
// grey smear at that scale; 14px was the first size that stayed readable.
// 0.012 (1.2% of output height) sits between those two bands — it equals
// ~13px on a 1080p frame — and clears every built-in component default
// already shipped (table/terminal/codeblock/pill_nav = 14px, badge `md` =
// 14px, kbd = 14px, tooltip = 13px), so it does not fire on scenarios that
// already validate clean today. Expressing it as a fraction of output
// height (rather than an absolute px count) makes the same *visual* size
// get flagged on a 4K or vertical-format canvas too.
pub const MIN_LEGIBLE_FONT_RATIO: f32 = 0.012;

/// [`MIN_LEGIBLE_FONT_RATIO`] evaluated at a fixed 1920×1080 reference
/// canvas (≈12.96px) — `text-autofit`'s shrink floor.
///
/// This is deliberately **not** `MIN_LEGIBLE_FONT_RATIO * scenario.video.
/// height`, unlike `check_legibility`'s own per-scenario check. Reason:
/// `text-autofit` must resolve to the *identical* px value wherever it's
/// computed (`TextIntrinsic::measure`, which runs pre-layout inside
/// `box_builder.rs`, and `Text`/`GradientText`'s painters, which run
/// post-layout with a real `PaintCtx`) — see the measure/paint parity
/// argument on `CssStyle::text_autofit`. `box_builder.rs` does not thread
/// the real `VideoConfig` down to where `TextIntrinsic` is constructed (out
/// of this workstream's file scope), so the painter side cannot be allowed
/// to use the real, more accurate `ctx.video_height` either — doing so would
/// silently reintroduce exactly the measure-vs-paint divergence this
/// workstream exists to prevent, just relocated from "the box" to "the
/// floor". Pinning both sides to the same fixed reference trades per-canvas
/// precision (a vertical 1080×2256 scenario's *true* 1.2%-of-height floor is
/// larger than this) for the non-negotiable guarantee that they agree. This
/// does not weaken `check_legibility` itself: that check still runs
/// independently, against the real canvas, on whatever `font-size` was
/// authored — it has no visibility into `text-autofit`'s runtime output
/// either way (see the workstream report's "non traité" list).
pub const TEXT_AUTOFIT_MIN_FONT_PX: f32 = MIN_LEGIBLE_FONT_RATIO * 1080.0;

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
    /// When `true` on `text`/`gradient_text`, the effective `font-size` is
    /// shrunk (never grown) until the content fits the box it was assigned,
    /// instead of overflowing it. This is what lets an author declare "this
    /// text must fit here" and closes `ContentOverflowsBox` as a possible
    /// validator failure for that node — see `apply_fixes`
    /// (`rustmotion-cli/src/commands/validate.rs`)'s comment on why it
    /// deliberately refuses to auto-fix that violation today: growing the
    /// box, shrinking the font, and shortening the copy are all legitimate
    /// fixes, and picking one was never this tool's call to make silently.
    /// `text-autofit` removes that ambiguity by having the *author* pick
    /// "shrink the font" up front.
    ///
    /// **Which box.** Two independent axes, each opt-in on its own:
    /// - *Width*: the box's resolved width — its own `width`/`max-width` if
    ///   set, else whatever it inherited from its parent (exactly the value
    ///   `text`/`gradient_text` already wrap against — see
    ///   `TextIntrinsic`/`Text::paint`). Always present once the node is
    ///   laid out, so the width axis is always a candidate for shrinking.
    /// - *Height*: only when the node's own box resolves to a **definite**
    ///   height (an explicit `height`, or a parent that hands it one, e.g. a
    ///   fixed `flex-basis`) — never an implicit/inherited one. A box that
    ///   grows to fit its content has, by construction, nothing to overflow
    ///   on the height axis, so there is nothing to shrink for. See
    ///   `TextIntrinsic::measure` / `Text::paint`'s `content_height` for
    ///   exactly how this is read (the same taffy-resolved, padding/border-
    ///   already-subtracted content-box value on both the pre-layout measure
    ///   path and the post-layout paint path — this is what guarantees the
    ///   two agree on the target, not just the algorithm).
    ///
    /// **`white-space: nowrap`.** Does not change: nowrap still means "never
    /// break this into multiple lines". `text-autofit` composes with it
    /// rather than overriding it — a `nowrap` line combined with
    /// `text-autofit: true` shrinks the *one* line until it fits the box's
    /// width (this is `text`/`gradient_text`'s answer to Remotion's
    /// `fitText()`), it does not start wrapping.
    ///
    /// **`auto_scroll`** (`codeblock`/`terminal`). Unrelated: `text-autofit`
    /// is only read by `text`/`gradient_text`'s own painter/intrinsic —
    /// `codeblock`/`terminal` never look at this field, so there is no
    /// precedence to resolve between the two; `auto_scroll` keeps scrolling
    /// (never shrinking) exactly as documented in `CLAUDE.md`.
    ///
    /// **The floor.** Never shrinks below [`TEXT_AUTOFIT_MIN_FONT_PX`] — the
    /// same calibrated legibility ratio `check_legibility`
    /// (`rustmotion-cli/src/commands/geometry.rs`) already enforces, not a
    /// new threshold. If the content still doesn't fit at the floor, the
    /// floor size is used anyway (illegible-but-smallest beats an even
    /// larger overflow) and the geometry validator's `ContentOverflowsBox`
    /// still fires — `text-autofit` narrows that failure class, it does not
    /// silence it.
    pub text_autofit: Option<bool>,

    // ---- Visual ----
    pub background: Option<Background>,
    pub border_radius: Option<BorderRadius>,
    pub box_shadow: Option<Vec<BoxShadow>>,
    pub text_shadow: Option<Vec<TextShadow>>,
    pub opacity: Option<f32>,
    pub mix_blend_mode: Option<BlendMode>,
    pub clip_path: Option<ClipPath>,
    /// Gradient-colored border painted instead of `border` when present.
    /// `{ "colors": [...], "width": 2, "angle": 0 }` — angle follows the same
    /// convention as `background` linear gradients.
    pub gradient_border: Option<GradientBorder>,

    // ---- Legacy compat (accepted, never rendered — validator warns) ----
    /// Deprecated: use `backdrop-filter: [{ "fn": "blur", "radius": N }]`.
    pub backdrop_blur: Option<f32>,
    /// Deprecated: use `box-shadow` with `"inset": true`.
    pub inner_shadow: Option<InnerShadow>,

    // ---- Filters / effects ----
    pub filter: Option<Vec<FilterFn>>,
    pub backdrop_filter: Option<Vec<FilterFn>>,

    // ---- Transform ----
    pub transform: Option<Vec<TransformFn>>,
    pub transform_origin: Option<TransformOrigin>,
    pub perspective: Option<Length>,
    pub perspective_origin: Option<TransformOrigin>,

    // ---- Scene-camera parallax ----
    /// Parallax plane depth for the scene camera (issue #90). 0 = locked
    /// plane (the camera does not affect it), 1 = normal plane (default),
    /// above 1 = amplified foreground. v1: effective on direct children of
    /// the scene root only (each top-level child is one plane whose depth
    /// governs its whole subtree). Not inherited via cascade.
    pub depth: Option<f32>,

    // ---- Overflow / stacking ----
    pub overflow: Option<Overflow>,
    pub overflow_x: Option<Overflow>,
    pub overflow_y: Option<Overflow>,
    pub z_index: Option<i32>,
    pub visibility: Option<Visibility>,

    // ---- Animation ----
    #[serde(default, deserialize_with = "deserialize_animation_effects")]
    pub animation: Vec<AnimationEffect>,
    /// Smoothing for `timeline` style-state changes. Supported properties:
    /// `opacity`, and `color` on text/counter; everything else snaps at the
    /// step's `at`.
    pub transition: Option<StyleTransition>,

    // ---- Audio reactive binding ----
    #[serde(default)]
    pub audio_reactive: Option<AudioReactive>,
}

/// `transition` config: bare number = duration in seconds with the default
/// easing, or a `{ duration, easing }` object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum StyleTransition {
    Duration(f64),
    Config {
        duration: f64,
        #[serde(default = "default_transition_easing")]
        easing: crate::schema::EasingType,
    },
}

fn default_transition_easing() -> crate::schema::EasingType {
    crate::schema::EasingType::EaseInOut
}

// ---- Audio reactive binding ----

/// Bind a CSS property to audio analysis data.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct AudioReactive {
    /// Audio track src key. If None, uses the first entry in the cache.
    #[serde(default)]
    pub track: Option<String>,
    /// Which audio data source to use.
    pub source: AudioSource,
    /// Which CSS property to modulate.
    pub property: AudioReactiveProperty,
    /// Value when audio is at 0.
    pub min: f64,
    /// Value when audio is at 1.
    pub max: f64,
    /// Number of previous frames to average (0 = no smoothing).
    #[serde(default)]
    pub smoothing_frames: u32,
}

/// The audio data source for an AudioReactive binding.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum AudioSource {
    /// Overall amplitude (RMS). JSON: `"amplitude"`.
    Amplitude(AudioSourceTag),
    /// A specific frequency band (0..15). JSON: `{"band": 3}`.
    Band { band: u8 },
}

/// Tag-only variant for the amplitude source.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AudioSourceTag {
    Amplitude,
}

/// Which CSS property to modulate with audio.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AudioReactiveProperty {
    Opacity,
    Scale,
    TranslateY,
    Rotation,
}

impl StyleTransition {
    pub fn duration(&self) -> f64 {
        match self {
            StyleTransition::Duration(d) => *d,
            StyleTransition::Config { duration, .. } => *duration,
        }
    }

    pub fn easing(&self) -> crate::schema::EasingType {
        match self {
            StyleTransition::Duration(_) => default_transition_easing(),
            StyleTransition::Config { easing, .. } => easing.clone(),
        }
    }
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

    // ---- Context-aware typography resolution (issue #125 §2) ----
    //
    // `font_size_px_or`/`letter_spacing_px`/`line_height_for` above are the
    // context-free accessors ~50+ call sites across the engine use; they go
    // through `Length::px()`, which cannot resolve `%`/`em`/`rem`/`vw`/`vh`
    // (no `LengthContext` reaches them) and — as of this fix — warns loudly
    // instead of silently dropping to `0px` when the value actually is one
    // of those units (see `units::px_or_warn`). That is the "fail loudly"
    // half of issue #125 §2.
    //
    // The methods below are the "or work" half: given a `LengthContext`,
    // they resolve `%`/`em`/`rem`/`vw`/`vh` correctly for these three
    // properties specifically, honouring the CSS rule that `em` means two
    // different things depending on which property it's on:
    //   - on `font-size` itself, `em` is relative to the *parent's* computed
    //     font-size — i.e. `ctx.font_size` going in.
    //   - on `letter-spacing` / `line-height`, `em` is relative to the
    //     element's *own* (just-computed) font-size, not the parent's.
    // `typography_px_ctx` below resolves all three together and gets this
    // right by re-deriving the context between steps; the three individual
    // methods are the building blocks for callers that need only one value,
    // or that already have the right `ctx.font_size` for what they're
    // resolving.
    //
    // What is NOT fixed by this, and is explicitly out of scope for this
    // workstream (file allowlist: renderer/text.rs, css/units.rs,
    // css/style.rs — not css/cascade.rs): `cascade::inherit_from` copies an
    // inherited `font-size` down the tree as the raw, unresolved `Length` —
    // not a resolved px value. So today nothing walks the tree computing
    // "the actual parent font-size in px" to feed as `ctx.font_size` when
    // resolving a child's `em` font-size; a caller that plugs in some other
    // value (a default, the root font-size, whatever's convenient) gets a
    // *technically* resolved but *semantically wrong* base for that one
    // case. `rem` (always relative to a single scenario-wide root, not a
    // per-ancestor chain) and `vw`/`vh` (relative to the real viewport) do
    // NOT have this problem — they are fully correct via `ctx.root_font_size`
    // / `ctx.viewport_*` regardless of cascade. `%` on `line-height` is
    // special-cased below against the *own* font-size per CSS, not
    // `ctx.parent_size`, so it isn't affected either. In short: `em`/`%` on
    // `font-size` need a cascade.rs fix to be fully correct end-to-end;
    // everything else these methods resolve is correct today.
    //
    // These are additive — nothing above changes signature, and nothing
    // currently in the engine calls these yet, since every existing call
    // site (`rustmotion-components/**`) is outside this workstream's file
    // scope. Wiring a real `LengthContext` (viewport dims from `PaintCtx`,
    // parent font-size from a resolved-cascade) into those call sites is the
    // integration step a sibling workstream (or a follow-up PR) needs to do
    // for relative units on type to actually reach rendered output.

    /// `font-size` resolved against `ctx`, correctly handling
    /// `%`/`em`/`rem`/`vw`/`vh` — unlike [`Self::font_size_px_or`]. `em`/`%`
    /// resolve against `ctx.font_size`, which the caller should set to the
    /// parent's *actual computed* font-size in px for correctness (see the
    /// module note above on why nothing does that yet).
    pub fn font_size_px_ctx(&self, ctx: &LengthContext, default: f32) -> f32 {
        self.font_size
            .as_ref()
            .and_then(|l| l.parse().resolve(ctx))
            .unwrap_or(default)
    }

    /// `letter-spacing` resolved against `ctx`, correctly handling
    /// `%`/`em`/`rem`/`vw`/`vh` — unlike [`Self::letter_spacing_px`]. Per
    /// CSS, `em` here means the element's *own* font-size, so pass a `ctx`
    /// whose `font_size` is the already-resolved own font-size (e.g. via
    /// [`Self::font_size_px_ctx`]), not the parent's — see
    /// [`Self::typography_px_ctx`] for a helper that gets this right
    /// automatically.
    pub fn letter_spacing_px_ctx(&self, ctx: &LengthContext) -> f32 {
        self.letter_spacing
            .as_ref()
            .and_then(|l| l.parse().resolve(ctx))
            .unwrap_or(0.0)
    }

    /// `line-height` resolved against `ctx`, correctly handling
    /// `%`/`em`/`rem`/`vw`/`vh` — unlike [`Self::line_height_for`].
    /// `LineHeight::Number` (unitless, e.g. `1.5`) is unaffected — it always
    /// means `n * font_size` regardless of any context. For
    /// `LineHeight::Length`, `%` is special-cased to CSS's actual rule for
    /// this property (relative to the element's *own* font-size, not
    /// `ctx.parent_size` like `%` normally means): a generic
    /// `ParsedLength::resolve` would silently resolve it against the wrong
    /// base otherwise. Same own-vs-parent `em` caveat as
    /// [`Self::letter_spacing_px_ctx`] applies.
    pub fn line_height_for_ctx(&self, font_size: f32, ctx: &LengthContext) -> f32 {
        match &self.line_height {
            Some(LineHeight::Number(n)) => n * font_size,
            Some(LineHeight::Length(lp)) => match lp.parse() {
                ParsedLength::Percent(p) => p / 100.0 * font_size,
                other => other.resolve(ctx).unwrap_or(font_size * 1.3),
            },
            _ => font_size * 1.3,
        }
    }

    /// Resolve `font-size`, `letter-spacing`, and `line-height` together in
    /// one call, honouring CSS's two different `em` bases (see the module
    /// note above `font_size_px_ctx`): `font-size`'s own `em` resolves
    /// against `ctx.font_size` (conventionally the parent's font-size),
    /// while `letter-spacing`'s and `line-height`'s `em` resolve against the
    /// just-computed *own* font-size, not `ctx.font_size` again. Returns
    /// `(font_size_px, letter_spacing_px, line_height_px)`.
    pub fn typography_px_ctx(
        &self,
        ctx: &LengthContext,
        default_font_size: f32,
    ) -> (f32, f32, f32) {
        let font_size = self.font_size_px_ctx(ctx, default_font_size);
        let own_ctx = LengthContext { font_size, ..*ctx };
        let letter_spacing = self.letter_spacing_px_ctx(&own_ctx);
        let line_height = self.line_height_for_ctx(font_size, &own_ctx);
        (font_size, letter_spacing, line_height)
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
///
/// Constat #6: `#[serde(untagged)]` tries variants in declaration order and
/// keeps the first that succeeds. `Length(LengthPercentage)` has its own
/// `String` catch-all variant that accepts *any* string — so with `Keyword`
/// declared after `Length` (as this used to be), `"max-content"` matched
/// `Length(String("max-content"))` before `Keyword` was ever tried:
/// `max-content`/`min-content`/`fit-content` were unreachable, dead schema.
/// `Keyword` must come before the `Length` catch-all; `Auto` before either
/// is fine since it needs an exact `"auto"` match nothing else claims first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Size {
    Auto(AutoKw),
    Keyword(SizeKeyword),
    Length(LengthPercentage),
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
///
/// Constat #2: `CssStyle` itself has `deny_unknown_fields`, which gives the
/// impression that any bad key under `style` is rejected — but one level
/// down, `Sides`'s four fields are all `#[serde(default)]` with no
/// `deny_unknown_fields` of its own. Since this is an untagged enum, a
/// well-meaning but unsupported shape like `{"horizontal": 20}` (the exact
/// form the LAYOUT `margin-left` rule teaches LLMs to reach for) fails to
/// match `Uniform` (not a scalar) and then matches `Sides` anyway — every
/// side defaults to 0, no error. `deny_unknown_fields` here closes that: an
/// object that isn't a recognised `{top,right,bottom,left}` shape now fails
/// to match either variant, and the untagged enum reports it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
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
///
/// Constat #1: every other composite in this file is kebab-case on the wire
/// (`box-shadow` -> `offset-x`/`offset-y`, `transform-origin` -> `x`/`y`,
/// etc. — see `rules/component-field-placement.md`). `Corners` used to be
/// the sole snake_case outlier (`top_left`/...), with no `deny_unknown_fields`
/// and every field defaulted — so the kebab form a CSS-literate author (or
/// LLM) naturally writes matched *zero* declared fields, and being an
/// untagged enum, serde didn't complain: it just produced `Corners` with
/// every corner at 0px, silently. `rename_all = "kebab-case"` makes kebab
/// the canonical wire form (matching every neighbour); `alias` keeps the
/// original snake_case working for any scenario already written that way;
/// `deny_unknown_fields` turns any other spelling (a genuine typo) into a
/// named parse error instead of a third silent zero.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged, deny_unknown_fields)]
pub enum BorderRadius {
    Uniform(LengthPercentage),
    Corners {
        #[serde(default, alias = "top_left")]
        #[serde(rename = "top-left")]
        top_left: LengthPercentage,
        #[serde(default, alias = "top_right")]
        #[serde(rename = "top-right")]
        top_right: LengthPercentage,
        #[serde(default, alias = "bottom_right")]
        #[serde(rename = "bottom-right")]
        bottom_right: LengthPercentage,
        #[serde(default, alias = "bottom_left")]
        #[serde(rename = "bottom-left")]
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

/// A single grid track (column or row) sizing function.
///
/// Variant order matters here: this is `#[serde(untagged)]`, and serde tries
/// each variant in declaration order, keeping the first that deserializes
/// successfully. `Length(LengthPercentage)` accepts *any* JSON number or
/// string (its own `String` fallback variant is a catch-all), so it must be
/// tried last — otherwise it silently swallows bare numbers (meant to be
/// `Fr`, matching the `flex-grow` convention) and keyword strings like
/// `"auto"` (meant to be `Keyword`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum GridTrack {
    /// Bare JSON number, e.g. `1` — a flex fraction, same convention as
    /// `flex-grow`. Equivalent to the string form `"1fr"`.
    Fr(f32),
    /// `"auto"` / `"min-content"` / `"max-content"`.
    Keyword(GridTrackKeyword),
    /// Any other length/percentage, including the explicit string form of a
    /// flex fraction (`"1fr"`), which `LengthPercentage::parse()` resolves
    /// to the same `ParsedLength::Fr` as the bare-number form above.
    Length(LengthPercentage),
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
///
/// Same class of bug as constat #6 on [`Size`], found while auditing this
/// file for other untagged enums with a catch-all before a specific variant:
/// `Length(LengthPercentage)`'s `String` fallback accepts any string, so
/// with `Keyword` declared after it, `"normal"` matched
/// `Length(String("normal"))` — which then resolves through
/// `Length::px()`/`.parse()` as an unparseable length, falling back to 0 —
/// instead of `Keyword(LineHeightKw::Normal)`. `Keyword` now comes first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum LineHeight {
    Number(f32),
    Keyword(LineHeightKw),
    Length(LengthPercentage),
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
    /// Deterministic film-grain noise. Works in both `filter` and
    /// `backdrop-filter` chains (frosted-glass grain).
    Noise {
        /// Grain strength in 0..1 (alpha of the noise layer). Default 0.15.
        #[serde(default = "default_noise_intensity")]
        intensity: f32,
        /// Perlin-noise seed — same seed ⇒ identical grain on every frame.
        #[serde(default = "default_noise_seed")]
        seed: u64,
    },
}

fn default_noise_intensity() -> f32 {
    0.15
}

fn default_noise_seed() -> u64 {
    42
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

    // ---- Grid track deserialization (issue #105) ----
    //
    // `GridTrack` is `#[serde(untagged)]`; these lock in which variant a
    // given JSON shape resolves to, since that resolution previously
    // silently swallowed both `Fr` and `Keyword` into `Length`.

    #[test]
    fn grid_track_bare_number_is_fr() {
        let json = r#"{ "grid-template-columns": [1, 1, 1] }"#;
        let s: CssStyle = serde_json::from_str(json).unwrap();
        let tracks = s.grid_template_columns.expect("tracks set");
        assert_eq!(tracks.len(), 3);
        for t in &tracks {
            assert!(matches!(t, GridTrack::Fr(n) if (*n - 1.0).abs() < f32::EPSILON));
        }
    }

    #[test]
    fn grid_track_string_fr_is_length_parsed_as_fr() {
        let json = r#"{ "grid-template-columns": ["1fr", "2fr"] }"#;
        let s: CssStyle = serde_json::from_str(json).unwrap();
        let tracks = s.grid_template_columns.expect("tracks set");
        match &tracks[0] {
            GridTrack::Length(lp) => {
                assert_eq!(lp.parse(), crate::css::units::ParsedLength::Fr(1.0))
            }
            other => panic!("expected Length(\"1fr\"), got {other:?}"),
        }
        match &tracks[1] {
            GridTrack::Length(lp) => {
                assert_eq!(lp.parse(), crate::css::units::ParsedLength::Fr(2.0))
            }
            other => panic!("expected Length(\"2fr\"), got {other:?}"),
        }
    }

    #[test]
    fn grid_track_keyword_strings() {
        let json = r#"{ "grid-template-columns": ["auto", "min-content", "max-content"] }"#;
        let s: CssStyle = serde_json::from_str(json).unwrap();
        let tracks = s.grid_template_columns.expect("tracks set");
        assert!(matches!(
            tracks[0],
            GridTrack::Keyword(GridTrackKeyword::Auto)
        ));
        assert!(matches!(
            tracks[1],
            GridTrack::Keyword(GridTrackKeyword::MinContent)
        ));
        assert!(matches!(
            tracks[2],
            GridTrack::Keyword(GridTrackKeyword::MaxContent)
        ));
    }

    #[test]
    fn grid_track_px_string_is_length() {
        let json = r#"{ "grid-template-columns": ["200px", "50%"] }"#;
        let s: CssStyle = serde_json::from_str(json).unwrap();
        let tracks = s.grid_template_columns.expect("tracks set");
        match &tracks[0] {
            GridTrack::Length(lp) => {
                assert_eq!(lp.parse(), crate::css::units::ParsedLength::Px(200.0))
            }
            other => panic!("expected Length(200px), got {other:?}"),
        }
        match &tracks[1] {
            GridTrack::Length(lp) => {
                assert_eq!(lp.parse(), crate::css::units::ParsedLength::Percent(50.0))
            }
            other => panic!("expected Length(50%), got {other:?}"),
        }
    }

    // ---- issue #125 §2: context-aware typography resolution ----

    fn style_with(font_size: &str, letter_spacing: &str, line_height: &str) -> CssStyle {
        let json = format!(
            r#"{{ "font-size": {font_size}, "letter-spacing": {letter_spacing}, "line-height": {line_height} }}"#
        );
        serde_json::from_str(&json).unwrap()
    }

    #[test]
    fn font_size_px_ctx_resolves_vw() {
        let s = style_with(r#""15.6vw""#, "0", "1");
        let ctx = LengthContext {
            viewport_width: 1920.0,
            ..Default::default()
        };
        // The exact regression from issue #125 §2: `font-size: "15.6vw"`
        // used to resolve to 0 via `.px()` (rendering nothing / a black
        // frame). Through a LengthContext it resolves correctly.
        assert_eq!(s.font_size_px_ctx(&ctx, 48.0), 15.6 / 100.0 * 1920.0);
        // The context-free accessor still can't do this — proving the two
        // are genuinely different code paths, not the same thing renamed.
        assert_eq!(s.font_size_px_or(48.0), 0.0);
    }

    #[test]
    fn font_size_px_ctx_resolves_rem_without_cascade_dependency() {
        let s = style_with(r#""2rem""#, "0", "1");
        let ctx = LengthContext {
            root_font_size: 20.0,
            ..Default::default()
        };
        // rem is relative to a single scenario-wide root font-size, not a
        // per-ancestor chain — no cascade.rs involvement needed for this to
        // be correct.
        assert_eq!(s.font_size_px_ctx(&ctx, 48.0), 40.0);
    }

    #[test]
    fn font_size_px_ctx_falls_back_to_default_when_unset() {
        let s = CssStyle::default();
        assert_eq!(s.font_size_px_ctx(&LengthContext::default(), 48.0), 48.0);
    }

    #[test]
    fn letter_spacing_px_ctx_resolves_own_em_not_parent_em() {
        // letter-spacing's `em` is relative to the *element's own*
        // font-size, not whatever `ctx.font_size` happened to be for
        // resolving font-size itself.
        let s = style_with("300", r#""-0.03em""#, "1");
        let own_ctx = LengthContext {
            font_size: 300.0, // the element's own resolved font-size
            ..Default::default()
        };
        assert!((s.letter_spacing_px_ctx(&own_ctx) - (-9.0)).abs() < 1e-4);
        // Context-free path can't resolve this at all (issue #125 §2): it
        // silently (now loudly, but still numerically) drops to 0, which is
        // byte-identical to a deliberate zero tracking.
        assert_eq!(s.letter_spacing_px(), 0.0);
    }

    #[test]
    fn line_height_percent_resolves_against_own_font_size_not_parent_size() {
        // CSS special case: `line-height: 50%` means 50% of the element's
        // own font-size, NOT 50% of `ctx.parent_size` like `%` means for
        // most other properties (width, padding, etc).
        let s = style_with("100", r#""50%""#, r#""50%""#);
        let ctx = LengthContext {
            parent_size: 1000.0, // deliberately different from font_size,
            // to prove `%` here does NOT fall through to the generic
            // percent-of-parent resolution.
            ..Default::default()
        };
        assert_eq!(s.line_height_for_ctx(100.0, &ctx), 50.0);
    }

    #[test]
    fn line_height_number_ignores_context_like_before() {
        let s = style_with("100", "0", "1.5");
        assert_eq!(
            s.line_height_for_ctx(100.0, &LengthContext::default()),
            150.0
        );
    }

    #[test]
    fn typography_px_ctx_resolves_all_three_with_correct_em_bases() {
        // font-size: 1.5em against a 200px parent font-size -> 300px own
        // font-size. letter-spacing/line-height's em must then use that
        // 300px *own* size, not the 200px parent size passed in via ctx.
        // line-height as a bare JSON number (unitless, `LineHeight::Number`)
        // — a quoted `"0.85"` would instead deserialize as a `Length`
        // string, which parses a bare numeric string as *pixels*
        // (`ParsedLength::Px`), not as the unitless multiplier CSS means;
        // that's an existing quirk of `LineHeight`'s untagged variants,
        // unrelated to this fix.
        let s = style_with(r#""1.5em""#, r#""-0.03em""#, "0.85");
        let ctx = LengthContext {
            font_size: 200.0, // parent's font-size, for font-size's own em
            ..Default::default()
        };
        let (font_size, letter_spacing, line_height) = s.typography_px_ctx(&ctx, 48.0);
        assert_eq!(font_size, 300.0);
        assert!(
            (letter_spacing - (300.0 * -0.03)).abs() < 1e-3,
            "letter-spacing em must resolve against the OWN 300px font-size, got {letter_spacing}"
        );
        assert_eq!(line_height, 300.0 * 0.85);
    }

    // ---- constat #1: border-radius per-corner kebab-case (RED first) ----

    #[test]
    fn border_radius_corners_accepts_kebab_case() {
        // This is the shape every sibling composite in this file uses
        // (box-shadow -> offset-x/offset-y, transform-origin -> x/y, etc.)
        // and the shape `rules/component-field-placement.md` teaches. Before
        // the fix, `BorderRadius::Corners`'s fields are literally
        // `top_left`/`top_right`/... with no kebab alias, so this kebab
        // object fails to match `Corners` (unknown fields) and, being all
        // `#[serde(default)]`, matches it anyway with every corner at 0 —
        // the untagged enum never reports an error, it just silently
        // produces radius 0.
        let json = r#"{ "border-radius": { "top-left": "12px", "top-right": "12px", "bottom-right": "4px", "bottom-left": "4px" } }"#;
        let s: CssStyle = serde_json::from_str(json).unwrap();
        match s.border_radius {
            Some(BorderRadius::Corners {
                top_left,
                top_right,
                bottom_right,
                bottom_left,
            }) => {
                assert_eq!(top_left.px(), 12.0, "top-left must be honoured, not 0");
                assert_eq!(top_right.px(), 12.0);
                assert_eq!(bottom_right.px(), 4.0);
                assert_eq!(bottom_left.px(), 4.0);
            }
            other => panic!("expected Corners, got {other:?}"),
        }
    }

    #[test]
    fn border_radius_corners_still_accepts_legacy_snake_case() {
        // Back-compat: any scenario already written with the old
        // snake_case field names must keep working identically.
        let json = r#"{ "border-radius": { "top_left": "8px", "top_right": "8px", "bottom_right": "8px", "bottom_left": "8px" } }"#;
        let s: CssStyle = serde_json::from_str(json).unwrap();
        assert_eq!(s.border_radius_px(), Some(8.0));
    }

    #[test]
    fn border_radius_corners_typo_is_a_named_error_not_a_silent_zero() {
        // A misspelled key must not silently resolve to Corners{0,0,0,0} —
        // it must be reported.
        let json = r#"{ "border-radius": { "topleft": "12px" } }"#;
        let err = serde_json::from_str::<CssStyle>(json).expect_err("typo must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("topleft")
                || msg.contains("border-radius")
                || msg.contains("BorderRadius"),
            "error must name the offending input, got: {msg}"
        );
    }

    // ---- constat #2: `Edges` (padding/margin) rejects unknown shapes (RED first) ----

    #[test]
    fn edges_rejects_unknown_object_shape_instead_of_defaulting_to_zero() {
        // `rules/margin-left-hack.md`-adjacent trap: an LLM reasoning in CSS
        // terms writes `{"horizontal": 20}` instead of the supported
        // `{"top":.., "right":.., "bottom":.., "left":..}` shape. Before the
        // fix, `Edges::Sides`'s four fields are all `#[serde(default)]` with
        // no `deny_unknown_fields`, so this object matches `Sides` anyway
        // with every side at 0 — silent, wrong padding instead of an error.
        let json = r#"{ "padding": { "horizontal": 20 } }"#;
        let err = serde_json::from_str::<CssStyle>(json)
            .expect_err("an unrecognised padding shape must be rejected, not silently zeroed");
        let msg = err.to_string();
        assert!(
            msg.contains("horizontal") || msg.contains("padding") || msg.contains("Edges"),
            "error must name the offending input, got: {msg}"
        );
    }

    #[test]
    fn edges_still_accepts_valid_per_side_object() {
        let json = r#"{ "padding": { "top": "10px", "right": "20px", "bottom": "10px", "left": "20px" } }"#;
        let s: CssStyle = serde_json::from_str(json).unwrap();
        assert_eq!(s.padding_px(), (10.0, 20.0, 10.0, 20.0));
    }

    #[test]
    fn edges_still_accepts_uniform_scalar() {
        let json = r#"{ "padding": "24px" }"#;
        let s: CssStyle = serde_json::from_str(json).unwrap();
        assert_eq!(s.padding_px(), (24.0, 24.0, 24.0, 24.0));
    }

    // ---- constat #6: `Size` untagged variant order (RED first) ----

    #[test]
    fn size_keyword_max_content_is_reachable() {
        // `Size` is `#[serde(untagged)]`: Auto, Length, Keyword in that
        // declared order (before the fix). `Length(LengthPercentage)`'s
        // `String` fallback variant accepts *any* string, so it is tried
        // (and succeeds) before `Keyword` is ever reached — `max-content` /
        // `min-content` / `fit-content` are dead schema. After the fix,
        // `Keyword` must be tried before the `Length` catch-all.
        for (kw, expected) in [
            ("max-content", SizeKeyword::MaxContent),
            ("min-content", SizeKeyword::MinContent),
            ("fit-content", SizeKeyword::FitContent),
        ] {
            let json = format!(r#"{{ "width": "{kw}" }}"#);
            let s: CssStyle = serde_json::from_str(&json).unwrap();
            assert_eq!(
                s.width,
                Some(Size::Keyword(expected)),
                "width: \"{kw}\" must resolve to Size::Keyword, not Size::Length(String(..))"
            );
        }
    }

    #[test]
    fn size_length_and_auto_are_unaffected_by_the_reorder() {
        let s: CssStyle = serde_json::from_str(r#"{ "width": "200px" }"#).unwrap();
        assert!(matches!(s.width, Some(Size::Length(_))));
        let s: CssStyle = serde_json::from_str(r#"{ "width": "50%" }"#).unwrap();
        assert!(matches!(s.width, Some(Size::Length(_))));
        let s: CssStyle = serde_json::from_str(r#"{ "width": "auto" }"#).unwrap();
        assert!(matches!(s.width, Some(Size::Auto(_))));
        let s: CssStyle = serde_json::from_str(r#"{ "width": 200 }"#).unwrap();
        assert!(matches!(s.width, Some(Size::Length(_))));
    }

    // ---- extra: `LineHeight` has the same catch-all-before-specific shape
    // as constat #6's `Size`, found while auditing this file for the same
    // bug class. Fixed alongside it (see the doc comment on `LineHeight`).

    #[test]
    fn line_height_keyword_normal_is_reachable() {
        let s: CssStyle = serde_json::from_str(r#"{ "line-height": "normal" }"#).unwrap();
        assert_eq!(
            s.line_height,
            Some(LineHeight::Keyword(LineHeightKw::Normal)),
            "line-height: \"normal\" must resolve to Keyword, not Length(String(\"normal\"))"
        );
    }

    #[test]
    fn line_height_number_and_length_are_unaffected_by_the_reorder() {
        let s: CssStyle = serde_json::from_str(r#"{ "line-height": 1.5 }"#).unwrap();
        assert!(matches!(s.line_height, Some(LineHeight::Number(_))));
        let s: CssStyle = serde_json::from_str(r#"{ "line-height": "24px" }"#).unwrap();
        assert!(matches!(s.line_height, Some(LineHeight::Length(_))));
    }

    // ---- border-radius: kebab-case is the canonical wire form on output ----

    #[test]
    fn border_radius_corners_serializes_as_kebab_case() {
        let s = CssStyle {
            border_radius: Some(BorderRadius::Corners {
                top_left: LengthPercentage::Px(1.0),
                top_right: LengthPercentage::Px(2.0),
                bottom_right: LengthPercentage::Px(3.0),
                bottom_left: LengthPercentage::Px(4.0),
            }),
            ..Default::default()
        };
        let json = serde_json::to_value(&s).unwrap();
        let br = &json["border-radius"];
        assert_eq!(br["top-left"], serde_json::json!(1.0));
        assert_eq!(br["top-right"], serde_json::json!(2.0));
        assert_eq!(br["bottom-right"], serde_json::json!(3.0));
        assert_eq!(br["bottom-left"], serde_json::json!(4.0));
        assert!(
            br.get("top_left").is_none(),
            "must not emit the legacy snake_case key any more"
        );
    }
}
