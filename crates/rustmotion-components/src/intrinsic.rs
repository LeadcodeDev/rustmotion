//! Intrinsic measurers for components whose box size depends on content.
//!
//! Uses the same Skia metrics that the painter uses, so the box reserved by
//! taffy matches the pixels actually drawn — measure-vs-paint mismatches
//! would otherwise cause text to wrap onto an extra line at paint time and
//! overflow into the next sibling.

use skia_safe::{Font, FontStyle as SkFontStyle, Typeface};

use rustmotion_core::css::style::{
    CssStyle, FontStyle as CssFontStyle, FontWeight as CssFontWeight, FontWeightKw, LineHeight,
    WhiteSpace, TEXT_AUTOFIT_MIN_FONT_PX,
};
use rustmotion_core::engine::box_tree::{AvailableSpace, IntrinsicMeasure};
use rustmotion_core::engine::renderer::{
    emoji_typeface, format_counter_value, measure_text_with_fallback, typeface_with_fallback,
    wrap_text_with_tracking,
};

use crate::badge::{Badge, BadgeSize};
use crate::caption::Caption;
use crate::counter::Counter;
use crate::gradient_text::GradientText;
use crate::kbd::Kbd;
use crate::text::Text;

// ─── Shared `font-size` context resolution (deployment of `font_size_px_ctx`
// / `typography_px_ctx`, css/style.rs, across every component that still
// resolved `font-size` with the context-free `font_size_px_or`) ───────────
//
// `font_size_px_or`/`.px()` cannot resolve `%`/`em`/`rem`/`vw`/`vh` — for a
// `Some(Length::String(_))` that parses as one of those units, `.px()` warns
// and returns `0.0`, and since the field itself is `Some`, the `_or`
// fallback default never kicks in either. A `text` with `"font-size":
// "2rem"` therefore measured *and* painted at 0px: `validate` passed (only
// warnings), but the rendered frame had no visible text (paint_pass's
// `height <= 0.0` guard skips the node once the intrinsic measures it at
// zero).
//
// `rustmotion_core::css::style::CssStyle::font_size_px_ctx` (and
// `typography_px_ctx`, which resolves `font-size`, `letter-spacing`, and
// `line-height` together, honouring CSS's two different `em` bases) already
// exist and are tested — nothing in the engine called them. These two
// helpers build the `LengthContext` every call site below feeds them,
// so the context-building logic lives in exactly one place instead of being
// copied into ~15 components.
use rustmotion_core::css::units::LengthContext;

/// `LengthContext` for resolving `font-size` (and, through
/// [`CssStyle::typography_px_ctx`], `letter-spacing`/`line-height` derived
/// from it) against a real, per-frame viewport. Use from `Painter::
/// paint_content` and friends, which have a real `PaintCtx` (`video_width`/
/// `video_height`) on hand.
///
/// `rem`/`vw`/`vh` resolve correctly through this. `em`/`%` on `font-size`
/// itself do not: per CSS they're relative to the *parent's* computed
/// font-size, but `cascade.rs` inherits `font-size` down the tree as a raw,
/// unresolved `Length`, not a resolved px value (see the module note above
/// `CssStyle::font_size_px_ctx`) — no caller in this workstream's scope can
/// supply the real cascaded value. `font_size: 16.0` here is the CSS root
/// default used as the best available stand-in; it makes `em`/`%` on
/// `font-size` *resolve* (no longer silently drop to 0px) without making
/// them *correct* against an actual parent font-size. Fixing that fully
/// needs a `cascade.rs` change, out of scope here.
pub fn font_size_ctx(viewport_width: f32, viewport_height: f32, parent_size: f32) -> LengthContext {
    LengthContext {
        viewport_width,
        viewport_height,
        parent_size,
        font_size: 16.0,
        root_font_size: 16.0,
    }
}

/// Same as [`font_size_ctx`], for the `Intrinsic` measurers in this module:
/// they run at `box_builder`/`geometry` construction time, before layout, so
/// there is no real per-frame viewport to hand (see the pre-existing note on
/// `TextIntrinsic::from_parts`, which has the same limitation for
/// `letter-spacing`/`line-height`). Falls back to the engine-wide default
/// 1920×1080 (same as `LengthContext::default()`) so `rem` — which does not
/// depend on the viewport at all — still resolves exactly, and `vw`/`vh` get
/// a reasonable non-zero approximation instead of silently dropping to 0.
/// This can diverge from what `Painter::paint_content` resolves via
/// [`font_size_ctx`] for `vw`/`vh` specifically, on videos that aren't
/// 1920×1080 — closing that fully needs the real `VideoConfig` threaded
/// through `box_builder.rs`/`geometry.rs`, both outside this workstream.
pub fn measure_time_font_size_ctx(parent_size: f32) -> LengthContext {
    font_size_ctx(1920.0, 1080.0, parent_size)
}

/// Skia-backed intrinsic measurer for [`Text`] (audit #10: despite the name
/// this module's doc header suggests, this uses `skia_safe::Font::
/// measure_str` via `engine::renderer::text`'s fallback-aware helpers — the
/// same primitives `Text::paint` draws with — not `engine::text::cosmic`,
/// which has no callers on the real render path at all; see that module's
/// doc comment).
pub struct TextIntrinsic {
    content: String,
    font_family: Option<String>,
    font_size: f32,
    line_height_resolved: f32,
    weight: u16,
    italic: bool,
    letter_spacing: f32,
    max_width: Option<f32>,
    wrap: bool,
    /// `style.text-autofit == Some(true)`, but only ever set by
    /// [`Self::from_text`] / [`GradientTextIntrinsic::from_gradient_text`] —
    /// see [`Self::with_autofit`]'s doc comment for why `from_parts`/
    /// `from_parts_with_wrap` (shared by `Caption`/`Kbd`/`Badge`/`Counter`,
    /// none of whose painters read `text-autofit`) must never set this from
    /// `style` directly.
    text_autofit: bool,
}

impl TextIntrinsic {
    /// M1: `white-space: nowrap|pre` disables wrapping — the geometry
    /// validator's `unwrappable_text_overflow`/`ContentOverflowsBox` checks
    /// (crates/rustmotion/src/cli/commands/geometry.rs) already branch on
    /// exactly this pair of variants and re-measure via this same
    /// `TextIntrinsic`, so the wrap decision here must match theirs exactly
    /// or the validator's assumption about what the renderer produces is
    /// false.
    pub fn from_text(text: &Text) -> Self {
        let wrap = !matches!(
            text.style.white_space,
            Some(WhiteSpace::Nowrap | WhiteSpace::Pre)
        );
        // Measure the *longest* label the text can ever show, not just the
        // first: a box sized for "Saved" would be overrun the moment a
        // `states` entry swapped in "Saving draft…", and the geometry
        // validator — which measures through here — would have signed off on
        // the overflow.
        let widest = text
            .all_labels()
            .max_by_key(|label| label.chars().count())
            .unwrap_or(&text.content);
        Self::from_parts_with_wrap(widest, &text.style, text.max_width, wrap)
            .with_autofit(matches!(text.style.text_autofit, Some(true)))
    }

    /// Opt this instance into `text-autofit`. Deliberately a separate,
    /// explicit step rather than something `from_parts`/`from_parts_with_wrap`
    /// read off `style` themselves: those two constructors are shared by
    /// every atomic/synthetic-style caller in this file (`Caption`, `Kbd`,
    /// `Badge`, `Counter`) whose *painters* have no idea `text-autofit`
    /// exists — if the flag leaked in through the shared style, `measure()`
    /// would shrink the reserved box for one of those while the painter
    /// went on drawing at the full requested size, which is exactly the
    /// measure-vs-paint divergence this feature exists to prevent, not
    /// reintroduce elsewhere. Only [`Self::from_text`] and
    /// [`GradientTextIntrinsic::from_gradient_text`] call this, matching the
    /// two painters (`Text`, `GradientText`) that actually implement it.
    pub fn with_autofit(mut self, on: bool) -> Self {
        self.text_autofit = on;
        self
    }

    /// Generic constructor shared by [`GradientText`]/[`Caption`] intrinsics,
    /// whose painters don't (yet) implement `white-space: nowrap` — kept
    /// wrap:true unconditionally so their measured size still matches what
    /// those painters actually draw.
    pub fn from_parts(content: &str, style: &CssStyle, max_width: Option<f32>) -> Self {
        // No *real* `LengthContext` (real viewport, real parent width) is
        // reachable here without changing this constructor's signature —
        // its only callers are `box_builder.rs` and
        // `rustmotion/src/cli/commands/geometry.rs`, both outside this
        // workstream's scope (box_builder.rs is a sibling's live file this
        // wave; the geometry validator re-measures via this exact type and
        // must keep agreeing with it byte-for-byte, so changing what it
        // needs to pass in is not a call to make unilaterally here).
        // `measure_time_font_size_ctx` falls back to the engine-wide default
        // viewport (1920×1080) for this reason — see its doc comment.
        //
        // `font-size` itself, and `letter-spacing`/`line-height`'s `em`/`%`
        // (relative to this element's *own*, just-resolved font-size — CSS
        // spec, also documented on `CssStyle::letter_spacing_px_ctx`/
        // `line_height_for_ctx`) are resolved together by
        // `typography_px_ctx`, which re-derives the right context between
        // the two steps. `Text`/`Caption`'s painters resolve the same three
        // properties with the real `PaintCtx`'s viewport (lot B, wave S), so
        // `rem` (viewport-independent) always agrees between measure and
        // paint; `vw`/`vh` can diverge on videos that aren't 1920×1080 —
        // closing that fully needs the real `VideoConfig` plumbed through
        // `box_builder.rs`/`geometry.rs`, still out of scope for the reasons
        // above.
        let base_ctx = measure_time_font_size_ctx(0.0);
        let (font_size, letter_spacing, line_height_resolved) =
            style.typography_px_ctx(&base_ctx, 48.0);
        Self {
            content: content.to_string(),
            font_family: style.font_family.clone(),
            font_size,
            line_height_resolved,
            weight: weight_to_u16(style.font_weight.as_ref()),
            italic: matches!(style.font_style, Some(CssFontStyle::Italic)),
            letter_spacing,
            max_width,
            wrap: true,
            text_autofit: false,
        }
    }

    /// Build with an explicit `wrap` override (used by atomic components like
    /// counter, kbd, badge that never wrap).
    pub fn from_parts_with_wrap(
        content: &str,
        style: &CssStyle,
        max_width: Option<f32>,
        wrap: bool,
    ) -> Self {
        let mut t = Self::from_parts(content, style, max_width);
        t.wrap = wrap;
        t
    }
}

impl IntrinsicMeasure for TextIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        let max_width = if let Some(w) = known.0 {
            Some(w)
        } else {
            let avail_w = match available.0 {
                AvailableSpace::Definite(w) => Some(w),
                AvailableSpace::MaxContent => None,
                AvailableSpace::MinContent => Some(0.0),
            };
            match (self.max_width, avail_w) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            }
        };

        let Some(typeface) = self.typeface() else {
            return (0.0, 0.0);
        };
        let wrap_at = if self.wrap { max_width } else { None };
        let (base_w, base_h) = wrap_and_measure(
            &self.content,
            &typeface,
            self.font_size,
            wrap_at,
            self.letter_spacing,
            self.line_height_resolved,
        );

        if !self.text_autofit {
            return (base_w, base_h);
        }

        // Height target: mirrors the `known`/`available` merge above for
        // width — taffy hands a leaf its own `known`/`available` height
        // already padding/border-subtracted (content-box space) whenever
        // the node's own box resolves to a *definite* height, exactly the
        // same protocol it uses for width. No separate hand-rolled read of
        // `style.height` here: reusing this signal is what guarantees this
        // agrees with `Text::paint`'s `content_height` (from the *same*
        // taffy-resolved `BoxLayout::content_box()`, post-layout) — see
        // `CssStyle::text_autofit`'s doc comment.
        let target_height = match known.1 {
            Some(h) => Some(h),
            None => match available.1 {
                AvailableSpace::Definite(h) => Some(h),
                AvailableSpace::MaxContent => None,
                AvailableSpace::MinContent => Some(0.0),
            },
        };

        let (final_size, final_ls, final_lh) = resolve_text_autofit(
            &self.content,
            &typeface,
            self.font_size,
            self.letter_spacing,
            self.line_height_resolved,
            self.wrap,
            max_width,
            target_height,
        );

        if final_size >= self.font_size {
            return (base_w, base_h);
        }
        let wrap_at = if self.wrap { max_width } else { None };
        wrap_and_measure(
            &self.content,
            &typeface,
            final_size,
            wrap_at,
            final_ls,
            final_lh,
        )
    }
}

impl TextIntrinsic {
    fn sk_font_style(&self) -> SkFontStyle {
        let slant = if self.italic {
            skia_safe::font_style::Slant::Italic
        } else {
            skia_safe::font_style::Slant::Upright
        };
        let weight = skia_safe::font_style::Weight::from(self.weight as i32);
        SkFontStyle::new(weight, skia_safe::font_style::Width::NORMAL, slant)
    }

    fn typeface(&self) -> Option<Typeface> {
        let family = self.font_family.as_deref().unwrap_or("Inter");
        typeface_with_fallback(family, self.sk_font_style()).ok()
    }
}

/// Wrap `content` at `font_size` (with `letter_spacing`/`line_height`
/// already resolved for that size) and return its `(max_line_width,
/// total_height)` — the single wrap+measure routine `TextIntrinsic::measure`
/// calls for both its base (requested-size) and, when `text-autofit` shrinks
/// it, its final (resolved-size) measurement, so the two never drift apart
/// from hand-duplicated logic.
fn wrap_and_measure(
    content: &str,
    typeface: &Typeface,
    font_size: f32,
    wrap_at: Option<f32>,
    letter_spacing: f32,
    line_height: f32,
) -> (f32, f32) {
    let font = Font::from_typeface(typeface.clone(), font_size);
    let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));
    // Tracking-aware wrap (issue #125 §1): matches the real `letter_spacing`
    // used to measure each line's width just below, so the box this
    // measurer reserves and what the painter (also tracking-aware) actually
    // paints agree on line count.
    let lines = wrap_text_with_tracking(content, &font, &emoji_font, wrap_at, letter_spacing);
    let mut max_w = 0.0f32;
    for line in &lines {
        max_w = max_w.max(measure_text_with_fallback(
            line,
            &font,
            &emoji_font,
            letter_spacing,
        ));
    }
    let line_count = lines.len().max(1) as f32;
    (max_w, line_count * line_height)
}

/// `text-autofit`'s shared shrink resolution — the single computation
/// `TextIntrinsic::measure` and `Text`/`GradientText`'s painters all call
/// with identical inputs, so the resolved size can never disagree between
/// the box taffy reserves and the pixels actually painted into it (see
/// `CssStyle::text_autofit`'s doc comment — this exact class of bug is what
/// this workstream exists to close, not reopen).
///
/// Pure and stateless: the same `(content, typeface, requested_font_size,
/// requested_letter_spacing, requested_line_height, wrap, box_width,
/// declared_height)` always produces the same `(font_size, letter_spacing,
/// line_height)`. That purity is *why* calling this fresh every paint call
/// (frame) is stable rather than something that needs caching — see the two
/// call sites' comments for what does and does not change frame to frame.
/// The one input this deliberately never sees is the paint-time typewriter
/// reveal (`AnimatedProperties::visible_chars_progress`): both call sites
/// pass the full, untruncated content, so a reveal-in-progress can't make
/// the resolved size drift as more characters become visible.
///
/// `letter_spacing`/`line_height` are rescaled proportionally with the
/// chosen font size (`requested * chosen/requested`) rather than
/// re-resolved from the original CSS declaration at each candidate size.
/// This matches CSS exactly for the common declarations (a unitless
/// `line-height` number, `%`/`em` line-height, or the engine's `1.3×`
/// default all scale linearly with font-size by definition) and is a
/// deliberate approximation for the rare case of an absolute
/// (`px`/`rem`/`vw`/`vh`) `line-height`/`letter-spacing`, which CSS says
/// should stay fixed regardless of font-size — getting that exactly right
/// needs threading the full `CssStyle` (not just its already-resolved
/// scalars) through both call sites, out of scope here.
///
/// `box_width`/`declared_height`: `None` means nothing to fit against on
/// that axis (an unconstrained box cannot overflow); returns
/// `requested_font_size` unchanged, without measuring anything, when both
/// are `None`.
#[allow(clippy::too_many_arguments)]
pub fn resolve_text_autofit(
    content: &str,
    typeface: &Typeface,
    requested_font_size: f32,
    requested_letter_spacing: f32,
    requested_line_height: f32,
    wrap: bool,
    box_width: Option<f32>,
    declared_height: Option<f32>,
) -> (f32, f32, f32) {
    if requested_font_size <= 0.0 || (box_width.is_none() && declared_height.is_none()) {
        return (
            requested_font_size,
            requested_letter_spacing,
            requested_line_height,
        );
    }
    let wrap_at = if wrap { box_width } else { None };
    let measure_at = |size: f32| -> (f32, f32) {
        let ratio = size / requested_font_size;
        wrap_and_measure(
            content,
            typeface,
            size,
            wrap_at,
            requested_letter_spacing * ratio,
            requested_line_height * ratio,
        )
    };
    let floor = TEXT_AUTOFIT_MIN_FONT_PX.min(requested_font_size);
    let final_size = shrink_to_fit(
        requested_font_size,
        floor,
        box_width,
        declared_height,
        measure_at,
    );
    if final_size >= requested_font_size {
        (
            requested_font_size,
            requested_letter_spacing,
            requested_line_height,
        )
    } else {
        let ratio = final_size / requested_font_size;
        (
            final_size,
            requested_letter_spacing * ratio,
            requested_line_height * ratio,
        )
    }
}

/// Binary-search the largest font size in `[floor_px, requested_font_size]`
/// whose `measure_at(size)` fits within `(target_width, target_height)`
/// (either bound `None` = no constraint on that axis). Assumes `measure_at`
/// is monotonically non-increasing as `size` shrinks — true for real text:
/// smaller glyphs measure narrower, and a fixed pixel wrap width can only
/// need the same or fewer lines as glyphs get smaller. 16 halvings of the
/// search range give sub-0.01px precision for any realistic font size — this
/// is a visual convenience, not a geometry-critical value, so that precision
/// is far more than needed.
///
/// Never returns below `floor_px`: if the content doesn't fit there either,
/// `floor_px` is returned anyway — illegible-but-smallest beats an even
/// larger overflow — and the caller's own overflow signal (the geometry
/// validator's `ContentOverflowsBox`) is left to fire. This function never
/// silences that; it only tries to make it unnecessary.
fn shrink_to_fit(
    requested_font_size: f32,
    floor_px: f32,
    target_width: Option<f32>,
    target_height: Option<f32>,
    mut measure_at: impl FnMut(f32) -> (f32, f32),
) -> f32 {
    let eps = 0.5;
    let fits = |w: f32, h: f32| {
        target_width.is_none_or(|tw| w <= tw + eps) && target_height.is_none_or(|th| h <= th + eps)
    };

    let (w0, h0) = measure_at(requested_font_size);
    if fits(w0, h0) {
        return requested_font_size;
    }

    let floor_px = floor_px.min(requested_font_size).max(0.1);
    if floor_px >= requested_font_size {
        return requested_font_size;
    }

    let (mut lo, mut hi) = (floor_px, requested_font_size);
    let (w_floor, h_floor) = measure_at(lo);
    if !fits(w_floor, h_floor) {
        // Doesn't fit even at the floor — stop there and let the caller's
        // own overflow check fire; see this function's doc comment.
        return lo;
    }
    for _ in 0..16 {
        let mid = (lo + hi) / 2.0;
        let (w, h) = measure_at(mid);
        if fits(w, h) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

fn weight_to_u16(w: Option<&CssFontWeight>) -> u16 {
    match w {
        Some(CssFontWeight::Keyword(FontWeightKw::Bold)) => 700,
        Some(CssFontWeight::Keyword(FontWeightKw::Bolder)) => 800,
        Some(CssFontWeight::Keyword(FontWeightKw::Lighter)) => 300,
        Some(CssFontWeight::Keyword(FontWeightKw::Normal)) | None => 400,
        Some(CssFontWeight::Number(n)) => (*n).clamp(1, 1000),
    }
}

/// Cosmic-text–backed intrinsic measurer for [`GradientText`] — same content
/// model as [`Text`] (a single string + style); the gradient is purely a
/// paint-time concern and doesn't change box dimensions.
pub struct GradientTextIntrinsic(TextIntrinsic);

impl GradientTextIntrinsic {
    pub fn from_gradient_text(t: &GradientText) -> Self {
        // max_width comes from CSS style.width if set as a fixed pixel value
        use rustmotion_core::css::style::Size as CSize;
        use rustmotion_core::css::units::LengthPercentage;
        let max_width = match &t.style.width {
            Some(CSize::Length(LengthPercentage::Px(v))) => Some(*v),
            _ => None,
        };
        // M1 follow-up (issue #109 review): gradient_text now word-wraps
        // like `text` (see `gradient_text.rs::paint`) — mirror the same
        // white-space: nowrap|pre rule here so measure and paint agree.
        let wrap = !matches!(
            t.style.white_space,
            Some(WhiteSpace::Nowrap | WhiteSpace::Pre)
        );
        Self(
            TextIntrinsic::from_parts_with_wrap(&t.content, &t.style, max_width, wrap)
                .with_autofit(matches!(t.style.text_autofit, Some(true))),
        )
    }
}

impl IntrinsicMeasure for GradientTextIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        self.0.measure(known, available)
    }
}

/// Intrinsic measurer for [`Caption`]. Concatenates the words with single
/// spaces and measures the result like a regular text run.
pub struct CaptionIntrinsic(TextIntrinsic);

impl CaptionIntrinsic {
    pub fn from_caption(c: &Caption) -> Self {
        let joined = c
            .words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        // M1 follow-up: `Highlight`/`Karaoke`/`KaraokePop` word-wrap all
        // words (see `caption.rs::paint`); `white-space: nowrap|pre` now
        // forces them onto one line there too — mirror it here. (`WordByWord`
        // /`WordPop` show one word at a time; wrapping is moot for those,
        // same as the existing kbd/counter/badge "atomic, never wraps"
        // components, so this doesn't need a mode-specific branch.)
        let wrap = !matches!(
            c.style.white_space,
            Some(WhiteSpace::Nowrap | WhiteSpace::Pre)
        );
        Self(TextIntrinsic::from_parts_with_wrap(
            &joined,
            &c.style,
            c.max_width,
            wrap,
        ))
    }
}

impl IntrinsicMeasure for CaptionIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        self.0.measure(known, available)
    }
}

/// Intrinsic measurer for [`Kbd`] — measures the key text plus the legacy
/// keyboard-cap padding (h ≈ font_size × 0.7, v ≈ font_size × 0.4) and
/// enforces a min-width of `font_size × 1.8`.
pub struct KbdIntrinsic {
    text: TextIntrinsic,
    h_padding: f32,
    v_padding: f32,
    min_width: f32,
}

impl KbdIntrinsic {
    pub fn from_kbd(k: &Kbd) -> Self {
        let fs = k
            .style
            .font_size_px_ctx(&measure_time_font_size_ctx(0.0), k.font_size);
        let synthetic_style = synthesize_text_style(&k.style, fs, "SF Mono");
        Self {
            text: TextIntrinsic::from_parts_with_wrap(&k.key, &synthetic_style, None, false),
            h_padding: fs * 0.7,
            v_padding: fs * 0.4,
            min_width: fs * 1.8,
        }
    }
}

impl IntrinsicMeasure for KbdIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        let (tw, th) = self.text.measure(known, available);
        let w = (tw + self.h_padding * 2.0).max(self.min_width);
        let h = th + self.v_padding * 2.0;
        (w, h)
    }
}

/// Intrinsic measurer for [`Counter`] — reserves space for the largest absolute
/// value the counter will display so layout never reflows during animation.
pub struct CounterIntrinsic(TextIntrinsic);

impl CounterIntrinsic {
    pub fn from_counter(c: &Counter) -> Self {
        let absmax = c.from.abs().max(c.to.abs());
        let signed = if c.from < 0.0 || c.to < 0.0 {
            -absmax
        } else {
            absmax
        };
        let display = format_counter_value(signed, c.decimals, &c.separator, &c.prefix, &c.suffix);
        // Counter is atomic: it never wraps.
        Self(TextIntrinsic::from_parts_with_wrap(
            &display, &c.style, None, false,
        ))
    }
}

impl IntrinsicMeasure for CounterIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        self.0.measure(known, available)
    }
}

/// Intrinsic measurer for [`crate::number_wheel::NumberWheel`].
///
/// Every digit column is as wide as the *widest* digit, because that is how
/// the painter lays the reels out — otherwise a figure that lands on `111`
/// would reserve a narrow box and then overflow it while a `0` rolls past.
/// Measuring the value once per possible digit and keeping the largest gives
/// exactly the painter's own `max over digits` per column, separators
/// included, without duplicating its layout arithmetic here.
pub struct NumberWheelIntrinsic(TextIntrinsic);

impl NumberWheelIntrinsic {
    pub fn from_number_wheel(w: &crate::number_wheel::NumberWheel) -> Self {
        let widest = (0..10)
            .map(|d| {
                let ch = char::from_digit(d, 10).expect("0..10 is a digit");
                w.value
                    .chars()
                    .map(|c| if c.is_ascii_digit() { ch } else { c })
                    .collect::<String>()
            })
            .max_by(|a, b| {
                let measure = |s: &str| {
                    TextIntrinsic::from_parts_with_wrap(s, &w.style, None, false)
                        .measure(
                            (None, None),
                            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
                        )
                        .0
                };
                measure(a)
                    .partial_cmp(&measure(b))
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or_else(|| w.value.clone());
        // A wheel is atomic: it never wraps.
        Self(TextIntrinsic::from_parts_with_wrap(
            &widest, &w.style, None, false,
        ))
    }
}

impl IntrinsicMeasure for NumberWheelIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        self.0.measure(known, available)
    }
}

/// Intrinsic measurer for [`Badge`] — measures the label text plus icon, gap,
/// and the size-derived horizontal/vertical padding.
pub struct BadgeIntrinsic {
    text: TextIntrinsic,
    h_padding: f32,
    v_padding: f32,
    icon_extra: f32,
    font_size: f32,
}

impl BadgeIntrinsic {
    pub fn from_badge(b: &Badge) -> Self {
        let (default_fs, h_pad, v_pad, icon_size) = badge_size_params(&b.badge_size);
        let font_size = b
            .style
            .font_size_px_ctx(&measure_time_font_size_ctx(0.0), default_fs);
        let ratio = font_size / default_fs;
        let h_padding = h_pad * ratio;
        let v_padding = v_pad * ratio;
        let icon_extra = if b.icon.is_some() {
            icon_size * ratio + 6.0 * ratio
        } else {
            0.0
        };

        let synthetic_style = synthesize_text_style(&b.style, font_size, "Inter");

        Self {
            text: TextIntrinsic::from_parts_with_wrap(&b.text, &synthetic_style, None, false),
            h_padding,
            v_padding,
            icon_extra,
            font_size,
        }
    }
}

impl IntrinsicMeasure for BadgeIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        let (tw, _th) = self.text.measure(known, available);
        let w = self.h_padding * 2.0 + tw + self.icon_extra;
        let h = self.v_padding * 2.0 + self.font_size * 1.3;
        (w, h)
    }
}

fn badge_size_params(s: &BadgeSize) -> (f32, f32, f32, f32) {
    // (font_size, h_padding, v_padding, icon_size) — matches badge.rs::params
    match s {
        BadgeSize::Sm => (12.0, 8.0, 4.0, 14.0),
        BadgeSize::Md => (14.0, 12.0, 6.0, 18.0),
        BadgeSize::Lg => (18.0, 16.0, 8.0, 22.0),
    }
}

/// Build a CssStyle for text measurement carrying just the typography fields
/// from `src`, with a forced `font-size` and `font-family` fallback.
fn synthesize_text_style(src: &CssStyle, font_size: f32, default_family: &str) -> CssStyle {
    use rustmotion_core::css::Length;
    let family = src
        .font_family
        .clone()
        .unwrap_or_else(|| default_family.to_string());
    CssStyle {
        font_size: Some(Length::Px(font_size)),
        font_family: Some(family),
        font_weight: src.font_weight.clone(),
        font_style: src.font_style,
        letter_spacing: src.letter_spacing.clone(),
        line_height: src.line_height.clone(),
        ..CssStyle::default()
    }
}

// Compatibility shim: keep an unused fn so old callers that referenced
// `LineHeight::Number` style helpers compile cleanly.
#[allow(dead_code)]
fn _line_height_unused(_: Option<&LineHeight>) {}

// ─────────────────────────────────────────────────────────────────────────────
// Terminal intrinsic measurer
// ─────────────────────────────────────────────────────────────────────────────

use crate::terminal::{
    resolve_typeface as resolve_terminal_typeface, Terminal, CHROME_HEIGHT,
    FONT_SIZE as TERM_FONT_SIZE, LINE_HEIGHT as TERM_LINE_HEIGHT, PADDING as TERM_PADDING,
};

/// Intrinsic measurer for [`Terminal`].
///
/// Natural size formula (matches the painter exactly):
/// - `line_height = ceil(font_size × TERM_LINE_HEIGHT / TERM_FONT_SIZE)`
/// - `height = chrome_height + 2 × TERM_PADDING + n_lines × line_height`
/// - `width` = widest line text (prefix + content) + 2 × TERM_PADDING
///
/// If the Skia font fails to load, returns (0, 0) so layout falls back to
/// whatever container constraints supply.
pub struct TerminalIntrinsic {
    line_height: f32,
    n_lines: usize,
    chrome_height: f32,
    padding: f32,
    /// Maximum measured text width across all lines (including prefix).
    max_line_width: f32,
}

impl TerminalIntrinsic {
    pub fn from_terminal(t: &Terminal) -> Self {
        let font_size = t
            .style
            .font_size_px_ctx(&measure_time_font_size_ctx(0.0), TERM_FONT_SIZE);
        let line_height = (font_size * TERM_LINE_HEIGHT / TERM_FONT_SIZE).ceil();
        let chrome_height = if t.show_chrome { CHROME_HEIGHT } else { 0.0 };

        // Measure each line (prefix + text) with the same Skia font the painter uses.
        let max_line_width = Self::measure_max_width(t, font_size);

        Self {
            line_height,
            n_lines: t.lines.len(),
            chrome_height,
            padding: TERM_PADDING,
            max_line_width,
        }
    }

    fn measure_max_width(t: &Terminal, font_size: f32) -> f32 {
        // Same resolver the painter calls — see `terminal::resolve_typeface`.
        // Measuring with one face and painting with another is how text ends up
        // overflowing a box the geometry pass has already approved.
        let Some(typeface) = resolve_terminal_typeface(&t.style) else {
            // Font unavailable (CI without fonts); return 0 — the layout will
            // be width-unconstrained and the container drives the size.
            return 0.0;
        };
        let font = Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));

        t.lines
            .iter()
            .map(|line| {
                let prefix = match line.line_type {
                    crate::terminal::TerminalLineType::Prompt => "$ ",
                    _ => "",
                };
                let full = format!("{}{}", prefix, line.text);
                measure_text_with_fallback(&full, &font, &emoji_font, 0.0)
            })
            .fold(0.0f32, f32::max)
    }
}

impl IntrinsicMeasure for TerminalIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        _available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        let w = known.0.unwrap_or(self.max_line_width + self.padding * 2.0);
        let h = known.1.unwrap_or(
            self.chrome_height + self.padding * 2.0 + self.n_lines as f32 * self.line_height,
        );
        (w, h)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Table intrinsic measurer
// ─────────────────────────────────────────────────────────────────────────────

use crate::table::{
    Table, DEFAULT_CELL_PADDING, DEFAULT_FONT_SIZE as TABLE_FONT_SIZE, DEFAULT_ROW_HEIGHT_RATIO,
};

/// Intrinsic measurer for [`Table`].
///
/// Natural size formula (matches the painter exactly):
/// - `row_height = font_size × DEFAULT_ROW_HEIGHT_RATIO`
/// - `height = (1 + row_count) × row_height`  (header + data rows)
/// - `width`: if `column_widths` are provided, their sum; otherwise each
///   column gets `max(header_text_width + 2 × cell_padding, min_col_width)`.
pub struct TableIntrinsic {
    row_height: f32,
    row_count: usize, // data rows only; header adds 1
    total_width: f32,
}

impl TableIntrinsic {
    pub fn from_table(t: &Table) -> Self {
        let font_size = t
            .style
            .font_size_px_ctx(&measure_time_font_size_ctx(0.0), TABLE_FONT_SIZE);
        let row_height = font_size * DEFAULT_ROW_HEIGHT_RATIO;

        let total_width = Self::compute_width(t, font_size);

        Self {
            row_height,
            row_count: t.rows.len(),
            total_width,
        }
    }

    fn compute_width(t: &Table, font_size: f32) -> f32 {
        // Explicit column widths provided → sum them.
        if let Some(widths) = &t.column_widths {
            if !widths.is_empty() {
                return widths.iter().sum();
            }
        }

        // Measure each header with the bold font; add 2× cell_padding per column.
        let font_style = skia_safe::FontStyle::bold();
        let family = t.style.font_family.as_deref().unwrap_or("Inter");
        let Ok(typeface) = typeface_with_fallback(family, font_style) else {
            // Font unavailable: fall back to col_count × a reasonable minimum.
            let col_count = t.headers.len().max(1) as f32;
            return col_count * (TABLE_FONT_SIZE * 8.0 + DEFAULT_CELL_PADDING * 2.0);
        };
        let font = Font::from_typeface(typeface, font_size);
        let emoji_font = emoji_typeface().map(|tf| Font::from_typeface(tf, font_size));
        let cell_padding = t.cell_padding;

        // Also consider data cell widths to size columns appropriately.
        let col_count = t.headers.len().max(1);
        let mut col_widths: Vec<f32> = vec![0.0; col_count];

        for (i, header) in t.headers.iter().enumerate() {
            let w = measure_text_with_fallback(header, &font, &emoji_font, 0.0);
            col_widths[i] = col_widths[i].max(w + cell_padding * 2.0);
        }
        for row in &t.rows {
            for (i, cell) in row.iter().enumerate() {
                if i >= col_count {
                    break;
                }
                let w = measure_text_with_fallback(cell, &font, &emoji_font, 0.0);
                col_widths[i] = col_widths[i].max(w + cell_padding * 2.0);
            }
        }

        col_widths.iter().sum()
    }
}

impl IntrinsicMeasure for TableIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        _available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        let w = known.0.unwrap_or(self.total_width);
        let h = known
            .1
            .unwrap_or((1 + self.row_count) as f32 * self.row_height);
        (w, h)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Codeblock intrinsic measurer
// ─────────────────────────────────────────────────────────────────────────────

use crate::codeblock::dimensions::compute_code_dimensions;
use crate::codeblock::highlight::resolve_monospace_font;
use crate::codeblock::Codeblock;
use rustmotion_core::css::style::{FontWeight as CssFontWeight2, FontWeightKw as CssFontWeightKw2};
use rustmotion_core::schema::FontWeight;

/// Intrinsic measurer for [`Codeblock`].
///
/// Reuses `compute_code_dimensions` (same function as the painter) to derive:
/// - `width  = max_line_width + gutter_width + pad_left + pad_right`
/// - `height = line_count × line_height + pad_top + pad_bottom + chrome_height`
///
/// Computed once at construction from the initial `code` string. If a state
/// transition widens the content at paint time, `auto_scroll` handles vertical
/// overflow without needing the intrinsic to re-run.
pub struct CodeblockIntrinsic {
    natural_width: f32,
    natural_height: f32,
}

impl CodeblockIntrinsic {
    pub fn from_codeblock(c: &Codeblock) -> Self {
        let font_family = c.style.font_family_or("JetBrains Mono");
        let font_size = c
            .style
            .font_size_px_ctx(&measure_time_font_size_ctx(0.0), 14.0);
        let font_weight = match &c.style.font_weight {
            Some(CssFontWeight2::Keyword(CssFontWeightKw2::Bold | CssFontWeightKw2::Bolder)) => {
                FontWeight::Bold
            }
            Some(CssFontWeight2::Number(n)) if *n >= 600 => FontWeight::Bold,
            Some(CssFontWeight2::Number(n)) => FontWeight::Weight(*n),
            _ => FontWeight::Normal,
        };

        let Some(font) = resolve_monospace_font(font_family, font_size, font_weight) else {
            return Self {
                natural_width: 0.0,
                natural_height: 0.0,
            };
        };

        let padding = {
            let (t, r, b, l) = c.style.padding_px();
            if t == 0.0 && r == 0.0 && b == 0.0 && l == 0.0 {
                (16.0, 16.0, 16.0, 16.0)
            } else {
                (t, r, b, l)
            }
        };

        let chrome_height = if c.chrome.as_ref().is_some_and(|ch| ch.enabled) {
            36.0
        } else {
            0.0
        };

        let dims = compute_code_dimensions(&c.code, &font, font_size, padding, chrome_height, c);

        Self {
            natural_width: dims.total_width,
            natural_height: dims.total_height,
        }
    }
}

impl IntrinsicMeasure for CodeblockIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        _available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        let w = known.0.unwrap_or(self.natural_width);
        let h = known.1.unwrap_or(self.natural_height);
        (w, h)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// RichText intrinsic measurer
// ─────────────────────────────────────────────────────────────────────────────

use crate::rich_text::{RichText, RichTextSpan};

/// Intrinsic measurer for [`RichText`].
///
/// M2: previously absent from `component_intrinsic` entirely, so a
/// `rich_text` with no explicit `width`/`height` laid out 0×0 and rendered
/// nothing. Reuses `RichText::compute_layout` — the exact same word-wrapped
/// line-breaking algorithm the painter uses — so the box taffy reserves
/// always matches what gets painted (same measure/paint-parity rationale as
/// [`TextIntrinsic`]).
///
/// Always measures the full (untruncated) content — a `visible_chars`
/// typewriter animation must not reflow layout as it plays.
pub struct RichTextIntrinsic {
    spans: Vec<RichTextSpan>,
    style: CssStyle,
    max_width: Option<f32>,
}

impl RichTextIntrinsic {
    pub fn from_rich_text(rt: &RichText) -> Self {
        Self {
            spans: rt.spans.clone(),
            style: rt.style.clone(),
            max_width: rt.max_width,
        }
    }
}

impl IntrinsicMeasure for RichTextIntrinsic {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32) {
        let max_width = if let Some(w) = known.0 {
            Some(w)
        } else {
            let avail_w = match available.0 {
                AvailableSpace::Definite(w) => Some(w),
                AvailableSpace::MaxContent => None,
                AvailableSpace::MinContent => Some(0.0),
            };
            match (self.max_width, avail_w) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, Some(b)) => Some(b),
                (None, None) => None,
            }
        };

        let layout =
            RichText::compute_layout(&self.spans, &self.style, 1920.0, 1080.0, max_width, -1.0);
        let line_count = layout.lines.len().max(1) as f32;
        (layout.max_width, line_count * layout.line_height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::css::style::CssStyle;
    use rustmotion_core::css::Length;
    use rustmotion_core::engine::box_tree::AvailableSpace;

    #[test]
    fn measure_returns_positive_size_for_non_empty_text() {
        let text = Text {
            content: "Hello World".into(),
            max_width: None,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(32.0)),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
            caret: None,
            states: Vec::new(),
            swap: None,
        };
        let m = TextIntrinsic::from_text(&text);
        let (w, h) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert!(w > 0.0, "width should be > 0, got {}", w);
        assert!(
            h > 30.0,
            "height should be roughly font_size * line_height, got {}",
            h
        );
    }

    #[test]
    fn wrapping_grows_height_when_max_width_constrained() {
        let text = Text {
            content: "the quick brown fox jumps over the lazy dog".into(),
            max_width: None,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(20.0)),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
            caret: None,
            states: Vec::new(),
            swap: None,
        };
        let m = TextIntrinsic::from_text(&text);
        let (_w_unwrapped, h_unwrapped) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        let (_w_wrapped, h_wrapped) = m.measure(
            (None, None),
            (AvailableSpace::Definite(80.0), AvailableSpace::MaxContent),
        );
        assert!(
            h_wrapped > h_unwrapped,
            "wrapped height ({}) should exceed unwrapped ({})",
            h_wrapped,
            h_unwrapped,
        );
    }

    #[test]
    fn empty_text_has_zero_width_but_one_line_height() {
        let text = Text {
            content: "".into(),
            max_width: None,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(24.0)),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
            caret: None,
            states: Vec::new(),
            swap: None,
        };
        let m = TextIntrinsic::from_text(&text);
        let (w, h) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert_eq!(w, 0.0);
        assert!(h > 0.0);
    }

    // ─── M1: white-space: nowrap/pre ────────────────────────────────────────

    fn nowrap_text(content: &str, white_space: Option<WhiteSpace>) -> Text {
        Text {
            content: content.into(),
            max_width: None,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(20.0)),
                white_space,
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
            caret: None,
            states: Vec::new(),
            swap: None,
        }
    }

    #[test]
    fn nowrap_ignores_a_constrained_width_and_stays_one_line() {
        let text = nowrap_text(
            "the quick brown fox jumps over the lazy dog",
            Some(WhiteSpace::Nowrap),
        );
        let m = TextIntrinsic::from_text(&text);
        let (w_unconstrained, h_unconstrained) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        let (w_constrained, h_constrained) = m.measure(
            (None, None),
            (AvailableSpace::Definite(80.0), AvailableSpace::MaxContent),
        );
        assert!(
            w_constrained > 80.0,
            "nowrap must ignore the 80px constraint, got width {}",
            w_constrained
        );
        assert_eq!(
            w_constrained, w_unconstrained,
            "nowrap width must equal the natural (unconstrained) width regardless of available space"
        );
        assert_eq!(
            h_constrained, h_unconstrained,
            "nowrap must always report a single line's height, constrained or not"
        );
    }

    #[test]
    fn pre_disables_wrap_exactly_like_nowrap() {
        let text = nowrap_text("this string is too long to fit", Some(WhiteSpace::Pre));
        let m = TextIntrinsic::from_text(&text);
        let (w, _h) = m.measure(
            (None, None),
            (AvailableSpace::Definite(80.0), AvailableSpace::MaxContent),
        );
        assert!(
            w > 80.0,
            "white-space: pre must also ignore the width constraint, got {}",
            w
        );
    }

    #[test]
    fn normal_white_space_still_wraps_at_a_constrained_width() {
        // Regression guard: making `nowrap`/`pre` real must not touch the
        // default (`normal`/unset) wrapping path.
        let wrapped = nowrap_text(
            "the quick brown fox jumps over the lazy dog",
            Some(WhiteSpace::Normal),
        );
        let unset = nowrap_text("the quick brown fox jumps over the lazy dog", None);
        for text in [wrapped, unset] {
            let m = TextIntrinsic::from_text(&text);
            let (w, _h) = m.measure(
                (None, None),
                (AvailableSpace::Definite(80.0), AvailableSpace::MaxContent),
            );
            assert!(
                w <= 80.0 + 0.5,
                "white-space: normal (or unset) must still wrap at an 80px constraint, got {}",
                w
            );
        }
    }

    // ─── M2: rich_text intrinsic ─────────────────────────────────────────────

    fn span(text: &str) -> RichTextSpan {
        RichTextSpan {
            text: text.into(),
            color: None,
            font_size: None,
            font_weight: None,
            font_family: None,
            font_style: None,
            letter_spacing: None,
        }
    }

    #[test]
    fn rich_text_intrinsic_is_non_zero_without_explicit_size() {
        // M2's core defect: rich_text had no `component_intrinsic` entry at
        // all, so it measured 0×0 unless the author guessed a width/height.
        let spans = vec![span("Hello "), span("world")];
        let style = CssStyle {
            font_size: Some(Length::Px(32.0)),
            ..Default::default()
        };
        let intrinsic = RichTextIntrinsic {
            spans,
            style,
            max_width: None,
        };
        let (w, h) = intrinsic.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert!(w > 0.0, "rich_text natural width must be > 0, got {}", w);
        assert!(h > 0.0, "rich_text natural height must be > 0, got {}", h);
    }

    #[test]
    fn rich_text_intrinsic_wraps_a_single_long_span_internally() {
        // M2's second ask: a long single span must wrap like any other text,
        // not just break at span boundaries (there is only one span here).
        let spans = vec![span(
            "the quick brown fox jumps over the lazy dog and keeps going",
        )];
        let style = CssStyle {
            font_size: Some(Length::Px(24.0)),
            ..Default::default()
        };
        let intrinsic = RichTextIntrinsic {
            spans,
            style,
            max_width: None,
        };
        let (_w_unconstrained, h_unconstrained) = intrinsic.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        let (w_constrained, h_constrained) = intrinsic.measure(
            (None, None),
            (AvailableSpace::Definite(150.0), AvailableSpace::MaxContent),
        );
        assert!(
            w_constrained <= 150.0 + 0.5,
            "wrapped width must fit the 150px constraint, got {}",
            w_constrained
        );
        assert!(
            h_constrained > h_unconstrained,
            "constraining width must add lines (wrap within the single span): {} vs {}",
            h_constrained,
            h_unconstrained
        );
    }

    #[test]
    fn rich_text_intrinsic_matches_compute_layout_used_by_the_painter() {
        // Measure/paint parity: the intrinsic must reuse the exact same
        // layout algorithm the painter does, or taffy could reserve a box
        // that doesn't match what gets drawn.
        let spans = vec![span("Total: "), span("42"), span(" items")];
        let style = CssStyle::default();
        let intrinsic = RichTextIntrinsic {
            spans: spans.clone(),
            style: style.clone(),
            max_width: None,
        };
        let (w, h) = intrinsic.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        let layout = RichText::compute_layout(&spans, &style, 1920.0, 1080.0, None, -1.0);
        assert_eq!(w, layout.max_width);
        assert_eq!(h, layout.lines.len().max(1) as f32 * layout.line_height);
    }

    // ─── M1 follow-up: gradient_text / caption honor white-space too ───────

    #[test]
    fn gradient_text_intrinsic_ignores_constrained_width_when_nowrap() {
        let gt = GradientText {
            content: "the quick brown fox jumps over the lazy dog".into(),
            colors: vec!["#3B82F6".into(), "#8B5CF6".into()],
            angle: 90.0,
            animate_angle: false,
            speed: 0.5,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(20.0)),
                white_space: Some(WhiteSpace::Nowrap),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
        };
        let m = GradientTextIntrinsic::from_gradient_text(&gt);
        let (w, h) = m.measure(
            (None, None),
            (AvailableSpace::Definite(80.0), AvailableSpace::MaxContent),
        );
        assert!(
            w > 80.0,
            "nowrap gradient_text must ignore the 80px constraint, got {}",
            w
        );
        // Single line: height should be one line, not several.
        let (_, h_unconstrained) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert_eq!(h, h_unconstrained);
    }

    #[test]
    fn gradient_text_intrinsic_wraps_by_default() {
        let gt = GradientText {
            content: "the quick brown fox jumps over the lazy dog".into(),
            colors: vec!["#3B82F6".into(), "#8B5CF6".into()],
            angle: 90.0,
            animate_angle: false,
            speed: 0.5,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(20.0)),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
        };
        let m = GradientTextIntrinsic::from_gradient_text(&gt);
        let (_w, h_unconstrained) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        let (w_constrained, h_constrained) = m.measure(
            (None, None),
            (AvailableSpace::Definite(80.0), AvailableSpace::MaxContent),
        );
        assert!(w_constrained <= 80.0 + 0.5);
        assert!(h_constrained > h_unconstrained);
    }

    #[test]
    fn caption_intrinsic_ignores_constrained_width_when_nowrap() {
        let caption = Caption {
            words: "the quick brown fox jumps over the lazy dog"
                .split_whitespace()
                .map(|w| rustmotion_core::schema::CaptionWord {
                    text: w.to_string(),
                    start: 0.0,
                    end: 10.0,
                })
                .collect(),
            active_color: "#FFFF00".into(),
            mode: Default::default(),
            max_width: Some(80.0),
            pill_color: None,
            style: CssStyle {
                font_size: Some(Length::Px(20.0)),
                white_space: Some(WhiteSpace::Nowrap),
                ..Default::default()
            },
            timing: Default::default(),
            timeline: Vec::new(),
            stagger: None,
        };
        let m = CaptionIntrinsic::from_caption(&caption);
        let (w, _h) = m.measure(
            (None, None),
            (AvailableSpace::Definite(80.0), AvailableSpace::MaxContent),
        );
        assert!(
            w > 80.0,
            "nowrap caption intrinsic must ignore the 80px constraint, got {}",
            w
        );
    }

    // ─── #2 / #5: em/% typography resolve against own font-size, not 0 ────

    fn text_with_style(content: &str, style: CssStyle) -> Text {
        Text {
            content: content.into(),
            max_width: None,
            timing: Default::default(),
            style,
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
            caret: None,
            states: Vec::new(),
            swap: None,
        }
    }

    #[test]
    fn line_height_percent_no_longer_collapses_the_box_to_zero_height() {
        // #2 reproduction: `line-height: "150%"` went through the
        // context-free `line_height_for`, which cannot resolve `%` and
        // silently fell back to 0 — the intrinsic then reported a
        // `line_count * 0.0 = 0` height, so `paint_pass.rs`'s `if height <=
        // 0.0 { return }` guard skipped painting the node (and its
        // subtree) entirely, even though `validate` reported success.
        use rustmotion_core::css::units::LengthPercentage;
        let text = text_with_style(
            "VISIBLE?",
            CssStyle {
                font_size: Some(Length::Px(60.0)),
                line_height: Some(LineHeight::Length(LengthPercentage::String("150%".into()))),
                ..Default::default()
            },
        );
        let m = TextIntrinsic::from_text(&text);
        let (_w, h) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert!(
            (h - 90.0).abs() < 0.5,
            "line-height: 150% of a 60px font-size must resolve to 90px (own font-size, per \
             CSS), got {h}"
        );
    }

    #[test]
    fn line_height_em_no_longer_collapses_the_box_to_zero_height() {
        use rustmotion_core::css::units::LengthPercentage;
        let text = text_with_style(
            "VISIBLE?",
            CssStyle {
                font_size: Some(Length::Px(60.0)),
                line_height: Some(LineHeight::Length(LengthPercentage::String("1.5em".into()))),
                ..Default::default()
            },
        );
        let m = TextIntrinsic::from_text(&text);
        let (_w, h) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert!(
            (h - 90.0).abs() < 0.5,
            "line-height: 1.5em of a 60px font-size must resolve to 90px, got {h}"
        );
        // Sanity: matches the already-correct unitless-number form exactly,
        // proving em and the bare-number multiplier agree.
        let numeric = text_with_style(
            "VISIBLE?",
            CssStyle {
                font_size: Some(Length::Px(60.0)),
                line_height: Some(LineHeight::Number(1.5)),
                ..Default::default()
            },
        );
        let (_w, h_numeric) = TextIntrinsic::from_text(&numeric).measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        assert_eq!(h, h_numeric);
    }

    #[test]
    fn letter_spacing_em_matches_the_equivalent_px_measurement() {
        // #5 reproduction: `letter-spacing: "1.2em"` at font-size 200
        // (=240px) went through the context-free `letter_spacing_px`, which
        // returns 0 for `em` — the intrinsic reserved a box as if tracking
        // were 0 while `Text::paint` (which already uses the `_ctx`
        // resolver) painted with the real 240px tracking, so `validate`'s
        // `unwrappable_text_overflow`/viewport checks (which re-measure via
        // this same intrinsic) never saw the real, wider painted width.
        let em_style = CssStyle {
            font_size: Some(Length::Px(200.0)),
            letter_spacing: Some(Length::String("1.2em".into())),
            white_space: Some(WhiteSpace::Nowrap),
            ..Default::default()
        };
        let px_style = CssStyle {
            font_size: Some(Length::Px(200.0)),
            letter_spacing: Some(Length::Px(240.0)),
            white_space: Some(WhiteSpace::Nowrap),
            ..Default::default()
        };
        let w_em = TextIntrinsic::from_text(&text_with_style("TRACKING", em_style))
            .measure(
                (None, None),
                (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
            )
            .0;
        let w_px = TextIntrinsic::from_text(&text_with_style("TRACKING", px_style))
            .measure(
                (None, None),
                (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
            )
            .0;
        assert!(
            (w_em - w_px).abs() < 1.0,
            "letter-spacing: 1.2em (font-size 200) must measure the same as the equivalent \
             240px value: em={w_em}, px={w_px}"
        );
        // And it must differ from the old (broken) zero-tracking width —
        // otherwise this test would pass vacuously even if em still
        // resolved to 0.
        let w_zero_tracking = TextIntrinsic::from_text(&text_with_style(
            "TRACKING",
            CssStyle {
                font_size: Some(Length::Px(200.0)),
                white_space: Some(WhiteSpace::Nowrap),
                ..Default::default()
            },
        ))
        .measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        )
        .0;
        assert!(
            w_em > w_zero_tracking + 100.0,
            "em tracking must measurably widen the line versus zero tracking: em={w_em}, \
             zero={w_zero_tracking}"
        );
    }

    // ─── text-autofit: `shrink_to_fit` (pure binary search, no Skia) ───────

    #[test]
    fn shrink_to_fit_is_a_noop_when_content_already_fits() {
        let calls = std::cell::RefCell::new(Vec::new());
        let size = shrink_to_fit(48.0, 12.0, Some(200.0), Some(100.0), |s| {
            calls.borrow_mut().push(s);
            (150.0, 80.0)
        });
        assert_eq!(size, 48.0);
        assert_eq!(
            *calls.borrow(),
            vec![48.0],
            "must measure only once (at the requested size) when it already fits"
        );
    }

    #[test]
    fn shrink_to_fit_is_a_noop_when_nothing_to_fit_against() {
        // Both axes unconstrained: no target to shrink for, regardless of
        // what `measure_at` reports.
        let size = shrink_to_fit(48.0, 12.0, None, None, |_| (99999.0, 99999.0));
        assert_eq!(size, 48.0);
    }

    #[test]
    fn shrink_to_fit_finds_a_size_that_fits_the_width_target() {
        // Fake linear model (width = size * 2), matching how real glyph
        // widths scale roughly linearly with font size.
        let target = 100.0;
        let size = shrink_to_fit(120.0, 5.0, Some(target), None, |s| (s * 2.0, 10.0));
        assert!(size < 120.0, "must have shrunk, got {size}");
        assert!(size * 2.0 <= target + 0.5, "resolved size must fit: {size}");
        // And it's close to the true boundary, not grossly under-shrunk: one
        // more px would no longer fit.
        assert!(
            (size + 1.0) * 2.0 > target + 0.5,
            "resolved size should be close to the fitting boundary, got {size}"
        );
    }

    #[test]
    fn shrink_to_fit_respects_both_axes_jointly() {
        // Width alone would allow a much bigger size than height alone —
        // the chosen size must satisfy the tighter of the two.
        let size = shrink_to_fit(100.0, 5.0, Some(1000.0), Some(20.0), |s| (s, s * 2.0));
        assert!(size * 2.0 <= 20.5, "must respect the height target: {size}");
        assert!(
            (size + 0.5) * 2.0 > 20.5,
            "should converge close to the height boundary, got {size}"
        );
    }

    #[test]
    fn shrink_to_fit_never_returns_below_the_floor() {
        // Content that never fits even at the floor: must stop exactly
        // there, not silence the overflow by continuing to shrink.
        let size = shrink_to_fit(120.0, 20.0, Some(10.0), None, |s| (s * 5.0, 10.0));
        assert_eq!(size, 20.0, "must stop exactly at the floor, not lower");
    }

    #[test]
    fn shrink_to_fit_is_deterministic_across_repeated_calls() {
        // Same inputs, same deterministic binary search → same output every
        // time. This is the purity property the temporal-stability argument
        // (see `resolve_text_autofit`'s doc comment) rests on: nothing here
        // depends on when or how many times it's called.
        let run = || shrink_to_fit(90.0, 10.0, Some(137.0), Some(64.0), |s| (s * 1.7, s * 0.9));
        let a = run();
        let b = run();
        assert_eq!(a, b);
    }

    // ─── text-autofit: `resolve_text_autofit` (real Skia fonts) ────────────

    fn inter_typeface() -> Typeface {
        typeface_with_fallback("Inter", SkFontStyle::normal()).expect("Inter resolves in tests")
    }

    #[test]
    fn resolve_text_autofit_shrinks_to_fit_a_width_target() {
        let typeface = inter_typeface();
        let content = "A very long headline that will not fit in this box";
        let requested = 80.0;
        let box_width = 300.0;
        let (fs, ls, lh) = resolve_text_autofit(
            content,
            &typeface,
            requested,
            0.0,
            requested * 1.3,
            false, // nowrap: single line
            Some(box_width),
            None,
        );
        assert!(fs < requested, "must shrink, got {fs}");
        assert!(
            fs >= TEXT_AUTOFIT_MIN_FONT_PX - 0.01,
            "must not shrink past the calibrated floor, got {fs}"
        );
        // Prove the resolved size actually fits when wrapped/measured the
        // same way the caller will — not just that a smaller number came out.
        let (w, _) = wrap_and_measure(content, &typeface, fs, None, ls, lh);
        assert!(
            w <= box_width + 0.5,
            "resolved size must actually fit: w={w}, target={box_width}"
        );
    }

    #[test]
    fn resolve_text_autofit_is_a_noop_when_it_already_fits() {
        let typeface = inter_typeface();
        let (fs, ls, lh) = resolve_text_autofit(
            "hi",
            &typeface,
            24.0,
            1.0,
            30.0,
            true,
            Some(1000.0),
            Some(1000.0),
        );
        assert_eq!(fs, 24.0);
        assert_eq!(ls, 1.0);
        assert_eq!(lh, 30.0);
    }

    #[test]
    fn resolve_text_autofit_never_goes_below_the_calibrated_floor() {
        let typeface = inter_typeface();
        // Absurdly small box: even the floor doesn't fit, but the function
        // must still stop exactly at the floor.
        let (fs, _, _) = resolve_text_autofit(
            "This sentence is far too long for a ten pixel wide box",
            &typeface,
            80.0,
            0.0,
            104.0,
            true,
            Some(10.0),
            Some(10.0),
        );
        assert!(
            (fs - TEXT_AUTOFIT_MIN_FONT_PX).abs() < 0.01,
            "expected exactly the floor ({TEXT_AUTOFIT_MIN_FONT_PX}), got {fs}"
        );
    }

    #[test]
    fn resolve_text_autofit_rescales_letter_spacing_and_line_height_proportionally() {
        let typeface = inter_typeface();
        let (fs, ls, lh) = resolve_text_autofit(
            "SHRINK ME PLEASE, THIS LINE IS QUITE LONG",
            &typeface,
            100.0,
            5.0,
            130.0,
            false,
            Some(150.0),
            None,
        );
        assert!(fs < 100.0, "sanity: must have shrunk, got {fs}");
        let ratio = fs / 100.0;
        assert!((ls - 5.0 * ratio).abs() < 1e-3);
        assert!((lh - 130.0 * ratio).abs() < 1e-3);
    }

    // ─── text-autofit: `TextIntrinsic` end to end ──────────────────────────

    fn autofit_text(content: &str, font_size: f32) -> Text {
        Text {
            content: content.into(),
            max_width: None,
            timing: Default::default(),
            style: CssStyle {
                font_size: Some(Length::Px(font_size)),
                text_autofit: Some(true),
                white_space: Some(WhiteSpace::Nowrap),
                ..Default::default()
            },
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
            caret: None,
            states: Vec::new(),
            swap: None,
        }
    }

    #[test]
    fn text_intrinsic_shrinks_when_autofit_is_on_and_the_box_is_too_narrow() {
        let text = autofit_text("the quick brown fox jumps over the lazy dog", 60.0);
        let m = TextIntrinsic::from_text(&text);
        let (w_unconstrained, _) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        // A box at half the natural width needs roughly a ~50% size
        // reduction — comfortably above the legibility floor for a 60px
        // request, so this exercises the "shrinks and fits" path distinctly
        // from `..._still_overflows_when_even_the_floor_does_not_fit` below
        // (which drives it all the way to the floor on purpose). Derived
        // from the actual measured natural width rather than a hardcoded
        // px guess, so it isn't sensitive to exactly which glyph widths
        // this font ships.
        let target = w_unconstrained / 2.0;
        let (w_constrained, _) = m.measure(
            (None, None),
            (AvailableSpace::Definite(target), AvailableSpace::MaxContent),
        );
        assert!(
            w_constrained <= target + 0.5,
            "autofit must shrink the nowrap line to fit {target}px, got {w_constrained}"
        );
        assert!(
            w_constrained < w_unconstrained,
            "must have actually shrunk from the natural width ({w_unconstrained}), got {w_constrained}"
        );
    }

    #[test]
    fn text_intrinsic_ignores_autofit_target_when_the_flag_is_off() {
        let mut text = autofit_text("the quick brown fox jumps over the lazy dog", 60.0);
        text.style.text_autofit = None;
        let m = TextIntrinsic::from_text(&text);
        let (w, _) = m.measure(
            (None, None),
            (AvailableSpace::Definite(200.0), AvailableSpace::MaxContent),
        );
        assert!(
            w > 200.0,
            "without text-autofit, nowrap must still bleed past the box exactly as before, got {w}"
        );
    }

    #[test]
    fn text_intrinsic_autofit_still_overflows_when_even_the_floor_does_not_fit() {
        let text = autofit_text(
            "This is an extremely long sentence that will not fit no matter how much the font shrinks",
            80.0,
        );
        let m = TextIntrinsic::from_text(&text);
        let (w, _) = m.measure(
            (None, None),
            (AvailableSpace::Definite(5.0), AvailableSpace::MaxContent),
        );
        assert!(
            w > 5.0,
            "must not silently report a fit that never actually happened, got {w}"
        );
    }

    #[test]
    fn caption_intrinsic_never_autofits_even_if_style_declares_it() {
        // Regression guard for the leak this feature must not reintroduce:
        // `Caption`'s painter has no idea `text-autofit` exists (only
        // `Text`/`GradientText`'s do), so its intrinsic must never shrink
        // because of it, even if the field is present in `style` — see
        // `TextIntrinsic::with_autofit`'s doc comment.
        let caption = Caption {
            words: "the quick brown fox jumps over the lazy dog"
                .split_whitespace()
                .map(|w| rustmotion_core::schema::CaptionWord {
                    text: w.to_string(),
                    start: 0.0,
                    end: 10.0,
                })
                .collect(),
            active_color: "#FFFF00".into(),
            mode: Default::default(),
            max_width: None,
            pill_color: None,
            style: CssStyle {
                font_size: Some(Length::Px(60.0)),
                text_autofit: Some(true),
                white_space: Some(WhiteSpace::Nowrap),
                ..Default::default()
            },
            timing: Default::default(),
            timeline: Vec::new(),
            stagger: None,
        };
        let m = CaptionIntrinsic::from_caption(&caption);
        let (w_unconstrained, _) = m.measure(
            (None, None),
            (AvailableSpace::MaxContent, AvailableSpace::MaxContent),
        );
        let (w_constrained, _) = m.measure(
            (None, None),
            (AvailableSpace::Definite(200.0), AvailableSpace::MaxContent),
        );
        assert_eq!(
            w_constrained, w_unconstrained,
            "caption must ignore text-autofit entirely (nowrap bleeds exactly as before)"
        );
    }
}
