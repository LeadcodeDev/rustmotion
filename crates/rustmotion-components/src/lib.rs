#![allow(deprecated)]
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
pub mod chart;
pub mod comparison;
pub mod connector;
pub mod container;
pub mod countdown;
pub mod counter;
pub mod cursor;
pub mod divider;
pub mod dot_map;
pub mod gauge;
pub mod gif;
pub mod gradient_text;
pub mod heatmap;
pub mod icon;
pub mod image;
pub mod kbd;
pub mod line;
pub mod list;
pub mod lottie;
pub mod marquee;
pub mod mockup;
pub mod number_wheel;
pub mod particle;
pub mod pill_nav;
pub mod pointer;
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
pub mod text;
pub mod timeline;
pub mod tooltip;
pub mod treemap;
pub mod video;
pub mod waveform;
pub mod world_bitmap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use rustmotion_core::css::CssStyle;
use rustmotion_core::traits::{Animatable, Painter, Styled, StyledMut, Timed};

pub use arrow::Arrow;
pub use audio_spectrum::AudioSpectrum;
pub use avatar::Avatar;
pub use avatar_group::AvatarGroup;
pub use badge::Badge;
pub use callout::Callout;
pub use caption::Caption;
pub use chart::Chart;
pub use comparison::Comparison;
pub use connector::Connector;
pub use container::ContainerComponent;
pub use countdown::Countdown;
pub use counter::Counter;
pub use cursor::Cursor;
pub use divider::Divider;
pub use dot_map::DotMap;
pub use gauge::Gauge;
pub use gif::Gif;
pub use gradient_text::GradientText;
pub use heatmap::Heatmap;
pub use icon::Icon;
pub use image::Image;
pub use kbd::Kbd;
pub use line::Line;
pub use list::List;
pub use lottie::Lottie;
pub use marquee::Marquee;
pub use mockup::Mockup;
pub use number_wheel::NumberWheel;
pub use particle::Particle;
pub use pill_nav::PillNav;
pub use pointer::Pointer;
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
pub use text::Text;
pub use timeline::Timeline;
pub use tooltip::Tooltip;
pub use treemap::Treemap;
pub use video::Video;
pub use waveform::Waveform;

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
        assert!(!child.is_flow());
    }

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
    /// Author-declared name for this node, unique within its own scene
    /// (issue #328). Read back by other nodes' expressions through
    /// `node("id", "prop")` — see `rustmotion_core::engine::deps`'s module
    /// doc for the per-frame dependency graph this feeds, and the "unique
    /// within its scene" constraint that graph enforces at build time
    /// (`DepsError::DuplicateId`). Optional: a node with no `id` simply
    /// cannot be referenced by another one's expressions, and is otherwise
    /// unaffected.
    #[serde(default)]
    pub id: Option<String>,
    /// Declares that this component's job is to extend past the frame edge
    /// (e.g. a radial glow used as a base layer). Top-level field, not a
    /// `style` property — `CssStyle` is `deny_unknown_fields` and belongs to
    /// no one this wave. Defaults to `false`: no existing scenario changes
    /// behaviour. Exempts only `viewport_overflow` and `animated_text_overflow`
    /// (see `crates/rustmotion/src/cli/commands/geometry.rs`); it does NOT
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
    Timeline(Timeline),
    Tooltip(Tooltip),
    Treemap(Treemap),
    #[serde(
        rename = "div",
        alias = "container",
        alias = "card",
        alias = "flex",
        alias = "grid",
        alias = "positioned"
    )]
    Container(ContainerComponent),
    Waveform(Waveform),
}

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
            Component::Timeline(c) => Some(c),
            Component::Tooltip(c) => Some(c),
            Component::Treemap(c) => Some(c),
            Component::Container(c) => Some(c),
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
            Component::Timeline(c) => Some(c),
            Component::Tooltip(c) => Some(c),
            Component::Treemap(c) => Some(c),
            Component::Container(c) => Some(c),
            Component::Caption(c) => Some(c),
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
            Component::Timeline(c) => c,
            Component::Tooltip(c) => c,
            Component::Treemap(c) => c,
            Component::Container(c) => c,
        }
    }

    pub fn as_painter(&self) -> Option<&dyn Painter> {
        match self {
            Component::AudioSpectrum(c) => Some(c),
            Component::Waveform(c) => Some(c),
            Component::Container(c) => Some(c),
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
            Component::Chart(c) => Some(c),
            Component::Line(c) => Some(c),
            Component::Arrow(c) => Some(c),
            Component::Connector(c) => Some(c),
        }
    }

    pub fn with_cascaded_style(&self, resolved: &CssStyle) -> Option<Component> {
        let is_typographic = match self {
            Component::Text(_)
            | Component::GradientText(_)
            | Component::Caption(_)
            | Component::RichText(_)
            | Component::Badge(_)
            | Component::Callout(_)
            | Component::Counter(_)
            | Component::Divider(_)
            | Component::Icon(_)
            | Component::Kbd(_)
            | Component::List(_)
            | Component::Marquee(_)
            | Component::NumberWheel(_)
            | Component::PillNav(_)
            | Component::Table(_)
            | Component::Tooltip(_) => true,
            Component::AudioSpectrum(_)
            | Component::Shape(_)
            | Component::Image(_)
            | Component::Svg(_)
            | Component::Video(_)
            | Component::Gif(_)
            | Component::Cursor(_)
            | Component::Connector(_)
            | Component::Avatar(_)
            | Component::AvatarGroup(_)
            | Component::Arrow(_)
            | Component::Chart(_)
            | Component::Comparison(_)
            | Component::Countdown(_)
            | Component::DotMap(_)
            | Component::Gauge(_)
            | Component::Heatmap(_)
            | Component::Line(_)
            | Component::Lottie(_)
            | Component::Mockup(_)
            | Component::Particle(_)
            | Component::Progress(_)
            | Component::QrCode(_)
            | Component::SuccessCheck(_)
            | Component::Pointer(_)
            | Component::Rating(_)
            | Component::Skeleton(_)
            | Component::Slider(_)
            | Component::Sparkline(_)
            | Component::Stat(_)
            | Component::Stepper(_)
            | Component::Switch(_)
            | Component::TagCloud(_)
            | Component::Timeline(_)
            | Component::Treemap(_)
            | Component::Container(_)
            | Component::Waveform(_) => false,
        };
        if !is_typographic {
            return None;
        }
        let value = serde_json::to_value(self).ok()?;
        let mut clone: Component = serde_json::from_value(value).ok()?;
        let style = match &mut clone {
            Component::Text(c) => c.style_config_mut(),
            Component::GradientText(c) => c.style_config_mut(),
            Component::Caption(c) => c.style_config_mut(),
            Component::RichText(c) => c.style_config_mut(),
            Component::Badge(c) => c.style_config_mut(),
            Component::Callout(c) => c.style_config_mut(),
            Component::Counter(c) => c.style_config_mut(),
            Component::Divider(c) => c.style_config_mut(),
            Component::Icon(c) => c.style_config_mut(),
            Component::Kbd(c) => c.style_config_mut(),
            Component::List(c) => c.style_config_mut(),
            Component::Marquee(c) => c.style_config_mut(),
            Component::NumberWheel(c) => c.style_config_mut(),
            Component::PillNav(c) => c.style_config_mut(),
            Component::Table(c) => c.style_config_mut(),
            Component::Tooltip(c) => c.style_config_mut(),
            _ => unreachable!("classified as typographic by the match above"),
        };
        rustmotion_core::css::cascade::inherit_from(resolved, style);
        Some(clone)
    }
}
