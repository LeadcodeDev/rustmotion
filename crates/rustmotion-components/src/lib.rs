pub mod box_builder;
pub mod intrinsic;
pub mod legacy_dispatch;

pub mod arrow;
pub mod audio_spectrum;
pub mod avatar;
pub mod avatar_group;
pub mod badge;
pub mod callout;
pub mod caption;
pub mod card;
pub mod chart;
pub mod codeblock;
pub mod comparison;
pub mod connector;
pub mod container;
pub mod countdown;
pub mod counter;
pub mod cursor;
pub mod divider;
pub mod dot_map;
pub mod flex;
pub mod gauge;
pub mod gif;
pub mod gradient_text;
pub mod grid;
pub mod heatmap;
pub mod icon;
pub mod image;
pub mod kbd;
pub mod line;
pub mod list;
pub mod lottie;
pub mod marquee;
pub mod mockup;
pub mod notification;
pub mod number_wheel;
pub mod particle;
pub mod pill_nav;
pub mod pointer;
pub mod positioned;
pub mod progress;
pub mod qrcode;
pub mod rating;
pub mod rich_text;
pub mod shape;
pub mod skeleton;
pub mod slider;
pub mod sparkline;
pub mod stat;
pub mod stepper;
pub mod success_check;
pub mod svg;
pub mod switch;
pub mod table;
pub mod tag_cloud;
pub mod terminal;
pub mod text;
pub mod timeline;
pub mod tooltip;
pub mod treemap;
pub mod video;
pub mod waveform;
pub mod world_bitmap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use rustmotion_core::traits::{Animatable, Painter, Styled, Timed};

pub use arrow::Arrow;
pub use audio_spectrum::AudioSpectrum;
pub use avatar::Avatar;
pub use avatar_group::AvatarGroup;
pub use badge::Badge;
pub use callout::Callout;
pub use caption::Caption;
pub use card::Card;
pub use chart::Chart;
pub use codeblock::Codeblock;
pub use comparison::Comparison;
pub use connector::Connector;
pub use container::ContainerComponent;
pub use countdown::Countdown;
pub use counter::Counter;
pub use cursor::Cursor;
pub use divider::Divider;
pub use dot_map::DotMap;
pub use flex::Flex;
pub use gauge::Gauge;
pub use gif::Gif;
pub use gradient_text::GradientText;
pub use grid::Grid;
pub use heatmap::Heatmap;
pub use icon::Icon;
pub use image::Image;
pub use kbd::Kbd;
pub use line::Line;
pub use list::List;
pub use lottie::Lottie;
pub use marquee::Marquee;
pub use mockup::Mockup;
pub use notification::Notification;
pub use number_wheel::NumberWheel;
pub use particle::Particle;
pub use pill_nav::PillNav;
pub use pointer::Pointer;
pub use positioned::Positioned;
pub use progress::Progress;
pub use qrcode::QrCode;
pub use rating::Rating;
pub use rich_text::RichText;
pub use shape::Shape;
pub use skeleton::Skeleton;
pub use slider::Slider;
pub use sparkline::Sparkline;
pub use stat::Stat;
pub use stepper::Stepper;
pub use success_check::SuccessCheck;
pub use svg::Svg;
pub use switch::Switch;
pub use table::Table;
pub use tag_cloud::TagCloud;
pub use terminal::Terminal;
pub use text::Text;
pub use timeline::Timeline;
pub use tooltip::Tooltip;
pub use treemap::Treemap;
pub use video::Video;
pub use waveform::Waveform;

// --- Position mode ---

/// Constat #8: `PositionMode::Named(String)` accepts any string, but
/// [`ChildComponent::absolute_position`] only ever treats the literal
/// `"absolute"` specially — every other value (including the CSS-legitimate
/// `"relative"`/`"static"`, which an LLM reasoning in CSS terms naturally
/// reaches for) silently drops `x`/`y`: the component is taken out of flow
/// (`is_flow()` is false for any `Some(position)`) but never receives an
/// absolute offset either, since only `"absolute"` is matched. `x`/`y` are
/// top-level sibling fields on `ChildComponent`, not on `PositionMode`
/// itself, so this can't detect *whether* they were actually set — only
/// that, if they were, they are about to be silently ignored. `"absolute"`
/// stays completely silent (the common, correct case); anything else warns.
pub fn is_recognized_position_name(s: &str) -> bool {
    s == "absolute"
}

#[derive(Debug, Clone, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum PositionMode {
    Absolute { x: f32, y: f32 },
    Named(String),
}

impl<'de> Deserialize<'de> for PositionMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Absolute { x: f32, y: f32 },
            Named(String),
        }
        Ok(match Raw::deserialize(deserializer)? {
            Raw::Absolute { x, y } => PositionMode::Absolute { x, y },
            Raw::Named(s) => {
                if !is_recognized_position_name(&s) && warn_once_for(&s) {
                    eprintln!(
                        "Warning: position: \"{s}\" is not \"absolute\" — this component-level \
                         `position` shorthand only honours the literal \"absolute\" (paired with \
                         `x`/`y`); any other value, including CSS-legitimate ones like \
                         \"relative\"/\"static\", is accepted but silently drops `x`/`y` instead \
                         of positioning the element (it still removes the component from flex \
                         flow). Use `style.position` for real CSS relative/static semantics."
                    );
                }
                PositionMode::Named(s)
            }
        })
    }
}

/// True the first time this exact `position` value is seen, false afterwards.
///
/// `render_scene_frame` calls `prepare_scene` — and therefore re-runs this
/// `Deserialize` over the whole scene tree — once **per frame**. An unguarded
/// warning here would print the same line once per offending component per
/// frame: over a thousand times on a 1200-frame render, drowning out anything
/// else on stderr. Keyed by the value rather than a plain `Once` so a scenario
/// with several distinct bad values still hears about each of them.
pub(crate) fn warn_once_for(value: &str) -> bool {
    use std::collections::HashSet;
    use std::sync::{Mutex, OnceLock};
    static SEEN: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SEEN.get_or_init(Default::default)
        .lock()
        .map(|mut seen| seen.insert(value.to_owned()))
        .unwrap_or(false)
}

impl Default for PositionMode {
    fn default() -> Self {
        Self::Absolute { x: 0.0, y: 0.0 }
    }
}

#[cfg(test)]
mod position_mode_tests {
    use super::*;

    // ---- constat #8 (RED first) ----

    #[test]
    fn absolute_is_recognized() {
        assert!(is_recognized_position_name("absolute"));
    }

    #[test]
    fn relative_and_static_and_typos_are_not_recognized() {
        for s in ["relative", "static", "fixed", "sticky", "Absolute", "abs"] {
            assert!(
                !is_recognized_position_name(s),
                "'{s}' must not be treated as the recognised \"absolute\" value"
            );
        }
    }

    #[test]
    fn absolute_object_form_still_carries_x_y() {
        let json =
            r#"{ "position": { "x": 10.0, "y": 20.0 }, "type": "shape", "shape": "circle" }"#;
        let child: ChildComponent = serde_json::from_str(json).unwrap();
        assert_eq!(child.absolute_position(), Some((10.0, 20.0)));
    }

    #[test]
    fn absolute_string_form_with_sibling_x_y_still_carries_them() {
        let json =
            r#"{ "position": "absolute", "x": 5.0, "y": 7.0, "type": "shape", "shape": "circle" }"#;
        let child: ChildComponent = serde_json::from_str(json).unwrap();
        assert_eq!(child.absolute_position(), Some((5.0, 7.0)));
    }

    #[test]
    fn relative_still_parses_but_drops_x_y_and_the_helper_flags_it() {
        // The legitimate-CSS trap named in constat #8: an LLM writes
        // `"position": "relative"` (valid CSS) with `x`/`y` alongside it,
        // expecting a positioned element. The parse must not fail — this is
        // legitimate JSON per the schema's own untagged catch-all — but the
        // coordinates are provably dropped (`absolute_position()` is
        // `None`), and `is_recognized_position_name` is the named,
        // independently testable signal the warning path uses to detect
        // this instead of staying silent.
        let json =
            r#"{ "position": "relative", "x": 5.0, "y": 7.0, "type": "shape", "shape": "circle" }"#;
        let child: ChildComponent = serde_json::from_str(json).unwrap();
        assert!(
            !is_recognized_position_name("relative"),
            "this is exactly the case the warning fires for"
        );
        assert_eq!(
            child.absolute_position(),
            None,
            "x/y are indeed dropped for a non-\"absolute\" position — this is the silent \
             behaviour being made loud, not a new regression"
        );
        // The component is still taken out of flow, same as before.
        assert!(!child.is_flow());
    }

    /// `prepare_scene` re-runs this `Deserialize` over the whole scene tree
    /// once per frame, so the warning must be deduplicated or a 1200-frame
    /// render prints it 1200 times. Distinct values still each get a line.
    #[test]
    fn the_warning_fires_once_per_distinct_value_not_once_per_frame() {
        let value = "position-value-used-only-by-this-test";
        assert!(warn_once_for(value), "first sighting must warn");
        for _ in 0..1000 {
            assert!(
                !warn_once_for(value),
                "re-parsing the same value must stay silent"
            );
        }
        assert!(
            warn_once_for("a-different-position-value-for-this-test"),
            "a different bad value must still get its own warning"
        );
    }

    #[test]
    fn no_position_set_is_a_normal_flow_child() {
        let json = r#"{ "type": "shape", "shape": "circle" }"#;
        let child: ChildComponent = serde_json::from_str(json).unwrap();
        assert!(child.is_flow());
        assert_eq!(child.absolute_position(), None);
    }
}

// --- Child wrapper ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ChildComponent {
    #[serde(flatten)]
    pub component: Component,
    #[serde(default)]
    pub position: Option<PositionMode>,
    #[serde(default)]
    pub x: Option<f32>,
    #[serde(default)]
    pub y: Option<f32>,
    #[serde(default, rename = "z-index")]
    pub z_index: Option<i32>,
    /// Declares that this component's job is to extend past the frame edge
    /// (e.g. a radial glow used as a base layer). Top-level field, not a
    /// `style` property — `CssStyle` is `deny_unknown_fields` and belongs to
    /// no one this wave. Defaults to `false`: no existing scenario changes
    /// behaviour. Exempts only `viewport_overflow` and `animated_text_overflow`
    /// (see `crates/rustmotion-cli/src/commands/geometry.rs`); it does NOT
    /// exempt `content_overflows_box` — content larger than its own box stays
    /// a reported defect regardless of `bleed`. Applies to this component
    /// only: a bled container does not suppress checks on its children, since
    /// each child is its own `ChildComponent` with its own `bleed` flag.
    #[serde(default)]
    pub bleed: bool,
}

impl ChildComponent {
    pub fn is_flow(&self) -> bool {
        self.position.is_none()
    }

    pub fn is_decorative(&self) -> bool {
        matches!(self.component, Component::Particle(_))
    }

    pub fn absolute_position(&self) -> Option<(f32, f32)> {
        match &self.position {
            Some(PositionMode::Absolute { x, y }) => Some((*x, *y)),
            Some(PositionMode::Named(s)) if s == "absolute" => {
                Some((self.x.unwrap_or(0.0), self.y.unwrap_or(0.0)))
            }
            _ => None,
        }
    }
}

// --- Component enum ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Component {
    AudioSpectrum(AudioSpectrum),
    Text(Text),
    Shape(Shape),
    Image(Image),
    Icon(Icon),
    Svg(Svg),
    Video(Video),
    Gif(Gif),
    Counter(Counter),
    Cursor(Cursor),
    Caption(Caption),
    Codeblock(Codeblock),
    Connector(Connector),
    Avatar(Avatar),
    AvatarGroup(AvatarGroup),
    Arrow(Arrow),
    Badge(Badge),
    Callout(Callout),
    Chart(Chart),
    Comparison(Comparison),
    Countdown(Countdown),
    Divider(Divider),
    DotMap(DotMap),
    Gauge(Gauge),
    GradientText(GradientText),
    Heatmap(Heatmap),
    Kbd(Kbd),
    Line(Line),
    List(List),
    Lottie(Lottie),
    Marquee(Marquee),
    Mockup(Mockup),
    Notification(Notification),
    Particle(Particle),
    PillNav(PillNav),
    #[serde(alias = "progress_bar")]
    Progress(Progress),
    QrCode(QrCode),
    NumberWheel(NumberWheel),
    SuccessCheck(SuccessCheck),
    Pointer(Pointer),
    Rating(Rating),
    Skeleton(Skeleton),
    Slider(Slider),
    Sparkline(Sparkline),
    Stat(Stat),
    Stepper(Stepper),
    Switch(Switch),
    RichText(RichText),
    Table(Table),
    TagCloud(TagCloud),
    Terminal(Terminal),
    Timeline(Timeline),
    Tooltip(Tooltip),
    Treemap(Treemap),
    Positioned(Positioned),
    Flex(Flex),
    Grid(Grid),
    Card(Card),
    #[serde(rename = "div", alias = "container")]
    Container(ContainerComponent),
    Waveform(Waveform),
}

// --- Dispatch helpers ---

impl Component {
    pub fn as_animatable(&self) -> Option<&dyn Animatable> {
        match self {
            Component::AudioSpectrum(c) => Some(c),
            Component::Waveform(c) => Some(c),
            Component::Text(c) => Some(c),
            Component::Shape(c) => Some(c),
            Component::Image(c) => Some(c),
            Component::Icon(c) => Some(c),
            Component::Svg(c) => Some(c),
            Component::Video(c) => Some(c),
            Component::Gif(c) => Some(c),
            Component::Counter(c) => Some(c),
            Component::Cursor(c) => Some(c),
            Component::Caption(c) => Some(c),
            Component::Codeblock(c) => Some(c),
            Component::Avatar(c) => Some(c),
            Component::AvatarGroup(c) => Some(c),
            Component::Arrow(c) => Some(c),
            Component::Connector(c) => Some(c),
            Component::Badge(c) => Some(c),
            Component::Callout(c) => Some(c),
            Component::Chart(c) => Some(c),
            Component::Comparison(c) => Some(c),
            Component::Countdown(c) => Some(c),
            Component::Divider(c) => Some(c),
            Component::DotMap(c) => Some(c),
            Component::Gauge(c) => Some(c),
            Component::GradientText(c) => Some(c),
            Component::Heatmap(c) => Some(c),
            Component::Kbd(c) => Some(c),
            Component::Line(c) => Some(c),
            Component::List(c) => Some(c),
            Component::Lottie(c) => Some(c),
            Component::Marquee(c) => Some(c),
            Component::Mockup(c) => Some(c),
            Component::Notification(c) => Some(c),
            Component::Particle(c) => Some(c),
            Component::PillNav(c) => Some(c),
            Component::Progress(c) => Some(c),
            Component::QrCode(c) => Some(c),
            Component::NumberWheel(c) => Some(c),
            Component::SuccessCheck(c) => Some(c),
            Component::Pointer(c) => Some(c),
            Component::Rating(c) => Some(c),
            Component::Skeleton(c) => Some(c),
            Component::Slider(c) => Some(c),
            Component::Sparkline(c) => Some(c),
            Component::Stat(c) => Some(c),
            Component::Stepper(c) => Some(c),
            Component::Switch(c) => Some(c),
            Component::RichText(c) => Some(c),
            Component::Table(c) => Some(c),
            Component::TagCloud(c) => Some(c),
            Component::Terminal(c) => Some(c),
            Component::Timeline(c) => Some(c),
            Component::Tooltip(c) => Some(c),
            Component::Treemap(c) => Some(c),
            Component::Flex(c) => Some(c),
            Component::Grid(c) => Some(c),
            Component::Card(c) => Some(c),
            Component::Container(c) => Some(c),
            Component::Positioned(c) => Some(c),
        }
    }

    pub fn as_timed(&self) -> Option<&dyn Timed> {
        match self {
            Component::AudioSpectrum(c) => Some(c),
            Component::Waveform(c) => Some(c),
            Component::Text(c) => Some(c),
            Component::Shape(c) => Some(c),
            Component::Image(c) => Some(c),
            Component::Icon(c) => Some(c),
            Component::Svg(c) => Some(c),
            Component::Video(c) => Some(c),
            Component::Gif(c) => Some(c),
            Component::Counter(c) => Some(c),
            Component::Cursor(c) => Some(c),
            Component::Codeblock(c) => Some(c),
            Component::Avatar(c) => Some(c),
            Component::AvatarGroup(c) => Some(c),
            Component::Arrow(c) => Some(c),
            Component::Connector(c) => Some(c),
            Component::Badge(c) => Some(c),
            Component::Callout(c) => Some(c),
            Component::Chart(c) => Some(c),
            Component::Comparison(c) => Some(c),
            Component::Countdown(c) => Some(c),
            Component::Divider(c) => Some(c),
            Component::DotMap(c) => Some(c),
            Component::Gauge(c) => Some(c),
            Component::GradientText(c) => Some(c),
            Component::Heatmap(c) => Some(c),
            Component::Kbd(c) => Some(c),
            Component::Line(c) => Some(c),
            Component::List(c) => Some(c),
            Component::Lottie(c) => Some(c),
            Component::Marquee(c) => Some(c),
            Component::Mockup(c) => Some(c),
            Component::Notification(c) => Some(c),
            Component::Particle(c) => Some(c),
            Component::PillNav(c) => Some(c),
            Component::Progress(c) => Some(c),
            Component::QrCode(c) => Some(c),
            Component::NumberWheel(c) => Some(c),
            Component::SuccessCheck(c) => Some(c),
            Component::Pointer(c) => Some(c),
            Component::Rating(c) => Some(c),
            Component::Skeleton(c) => Some(c),
            Component::Slider(c) => Some(c),
            Component::Sparkline(c) => Some(c),
            Component::Stat(c) => Some(c),
            Component::Stepper(c) => Some(c),
            Component::Switch(c) => Some(c),
            Component::RichText(c) => Some(c),
            Component::Table(c) => Some(c),
            Component::TagCloud(c) => Some(c),
            Component::Terminal(c) => Some(c),
            Component::Timeline(c) => Some(c),
            Component::Tooltip(c) => Some(c),
            Component::Treemap(c) => Some(c),
            Component::Flex(c) => Some(c),
            Component::Grid(c) => Some(c),
            Component::Card(c) => Some(c),
            Component::Container(c) => Some(c),
            Component::Caption(c) => Some(c),
            Component::Positioned(c) => Some(c),
        }
    }

    pub fn as_styled(&self) -> &dyn Styled {
        match self {
            Component::AudioSpectrum(c) => c,
            Component::Waveform(c) => c,
            Component::Text(c) => c,
            Component::Shape(c) => c,
            Component::Image(c) => c,
            Component::Icon(c) => c,
            Component::Svg(c) => c,
            Component::Video(c) => c,
            Component::Gif(c) => c,
            Component::Counter(c) => c,
            Component::Cursor(c) => c,
            Component::Caption(c) => c,
            Component::Codeblock(c) => c,
            Component::Avatar(c) => c,
            Component::AvatarGroup(c) => c,
            Component::Arrow(c) => c,
            Component::Connector(c) => c,
            Component::Badge(c) => c,
            Component::Callout(c) => c,
            Component::Chart(c) => c,
            Component::Comparison(c) => c,
            Component::Countdown(c) => c,
            Component::Divider(c) => c,
            Component::DotMap(c) => c,
            Component::Gauge(c) => c,
            Component::GradientText(c) => c,
            Component::Heatmap(c) => c,
            Component::Kbd(c) => c,
            Component::Line(c) => c,
            Component::List(c) => c,
            Component::Lottie(c) => c,
            Component::Marquee(c) => c,
            Component::Mockup(c) => c,
            Component::Notification(c) => c,
            Component::Particle(c) => c,
            Component::PillNav(c) => c,
            Component::Progress(c) => c,
            Component::QrCode(c) => c,
            Component::NumberWheel(c) => c,
            Component::SuccessCheck(c) => c,
            Component::Pointer(c) => c,
            Component::Rating(c) => c,
            Component::Skeleton(c) => c,
            Component::Slider(c) => c,
            Component::Sparkline(c) => c,
            Component::Stat(c) => c,
            Component::Stepper(c) => c,
            Component::Switch(c) => c,
            Component::RichText(c) => c,
            Component::Table(c) => c,
            Component::TagCloud(c) => c,
            Component::Terminal(c) => c,
            Component::Timeline(c) => c,
            Component::Tooltip(c) => c,
            Component::Treemap(c) => c,
            Component::Positioned(c) => c,
            Component::Flex(c) => c,
            Component::Grid(c) => c,
            Component::Card(c) => c,
            Component::Container(c) => c,
        }
    }

    /// Returns the Painter trait. All 51 components are migrated to the new
    /// pipeline; the dispatcher always uses Painter::paint_content.
    pub fn as_painter(&self) -> Option<&dyn Painter> {
        match self {
            Component::AudioSpectrum(c) => Some(c),
            Component::Waveform(c) => Some(c),
            Component::Card(c) => Some(c),
            Component::Container(c) => Some(c),
            Component::Flex(c) => Some(c),
            Component::Grid(c) => Some(c),
            Component::Positioned(c) => Some(c),
            Component::Divider(c) => Some(c),
            Component::Shape(c) => Some(c),
            Component::Image(c) => Some(c),
            Component::Icon(c) => Some(c),
            Component::Svg(c) => Some(c),
            Component::QrCode(c) => Some(c),
            Component::Gif(c) => Some(c),
            Component::Video(c) => Some(c),
            Component::Lottie(c) => Some(c),
            Component::Cursor(c) => Some(c),
            Component::Particle(c) => Some(c),
            Component::Mockup(c) => Some(c),
            Component::Text(c) => Some(c),
            Component::Caption(c) => Some(c),
            Component::Badge(c) => Some(c),
            Component::Kbd(c) => Some(c),
            Component::Callout(c) => Some(c),
            Component::Marquee(c) => Some(c),
            Component::TagCloud(c) => Some(c),
            Component::GradientText(c) => Some(c),
            Component::RichText(c) => Some(c),
            Component::Switch(c) => Some(c),
            Component::Slider(c) => Some(c),
            Component::NumberWheel(c) => Some(c),
            Component::SuccessCheck(c) => Some(c),
            Component::Pointer(c) => Some(c),
            Component::Rating(c) => Some(c),
            Component::Stepper(c) => Some(c),
            Component::Comparison(c) => Some(c),
            Component::Notification(c) => Some(c),
            Component::Tooltip(c) => Some(c),
            Component::PillNav(c) => Some(c),
            Component::List(c) => Some(c),
            Component::Skeleton(c) => Some(c),
            Component::Avatar(c) => Some(c),
            Component::AvatarGroup(c) => Some(c),
            Component::Timeline(c) => Some(c),
            Component::Progress(c) => Some(c),
            Component::Counter(c) => Some(c),
            Component::Countdown(c) => Some(c),
            Component::Gauge(c) => Some(c),
            Component::Sparkline(c) => Some(c),
            Component::Stat(c) => Some(c),
            Component::Heatmap(c) => Some(c),
            Component::Treemap(c) => Some(c),
            Component::DotMap(c) => Some(c),
            Component::Table(c) => Some(c),
            Component::Codeblock(c) => Some(c),
            Component::Terminal(c) => Some(c),
            Component::Chart(c) => Some(c),
            Component::Line(c) => Some(c),
            Component::Arrow(c) => Some(c),
            Component::Connector(c) => Some(c),
        }
    }
}
