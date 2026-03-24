pub mod arrow;
pub mod avatar;
pub mod avatar_group;
pub mod badge;
pub mod callout;
pub mod caption;
pub mod card;
pub mod chart;
pub mod codeblock;
pub mod connector;
pub mod container;
pub mod counter;
pub mod cursor;
pub mod divider;
pub mod flex;
pub mod gauge;
pub mod gif;
pub mod grid;
pub mod icon;
pub mod kbd;
pub mod line;
pub mod lottie;
pub mod marquee;
pub mod image;
pub mod mockup;
pub mod particle;
pub mod positioned;
pub mod progress;
pub mod qrcode;
pub mod rich_text;
pub mod shape;
pub mod skeleton;
pub mod sparkline;
pub mod stat;
pub mod svg;
pub mod table;
pub mod terminal;
pub mod text;
pub mod timeline;
pub mod tooltip;
pub mod video;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::traits::{Animatable, Container, Styled, Timed, Widget};

pub use arrow::Arrow;
pub use avatar::Avatar;
pub use avatar_group::AvatarGroup;
pub use badge::Badge;
pub use callout::Callout;
pub use caption::Caption;
pub use card::Card;
pub use chart::Chart;
pub use codeblock::Codeblock;
pub use connector::Connector;
pub use container::ContainerComponent;
pub use counter::Counter;
pub use cursor::Cursor;
pub use divider::Divider;
pub use flex::Flex;
pub use gauge::Gauge;
pub use gif::Gif;
pub use grid::Grid;
pub use icon::Icon;
pub use image::Image;
pub use kbd::Kbd;
pub use line::Line;
pub use lottie::Lottie;
pub use marquee::Marquee;
pub use mockup::Mockup;
pub use particle::Particle;
pub use positioned::Positioned;
pub use progress::Progress;
pub use qrcode::QrCode;
pub use rich_text::RichText;
pub use shape::Shape;
pub use skeleton::Skeleton;
pub use sparkline::Sparkline;
pub use stat::Stat;
pub use svg::Svg;
pub use table::Table;
pub use terminal::Terminal;
pub use text::Text;
pub use timeline::Timeline;
pub use tooltip::Tooltip;
pub use video::Video;

// --- Position mode ---

/// How a component is positioned within its parent container.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PositionMode {
    /// Absolute positioning with explicit coordinates.
    Absolute { x: f32, y: f32 },
    /// Named mode (only "absolute" supported, for use without coords).
    Named(String),
}

impl Default for PositionMode {
    fn default() -> Self {
        // Default is flow — represented by absence of position field.
        // This variant is never constructed for flow; the Option<PositionMode> is None.
        Self::Absolute { x: 0.0, y: 0.0 }
    }
}

// --- Child wrapper ---

/// Wrapper for a component inside a container, with per-child layout metadata.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct ChildComponent {
    #[serde(flatten)]
    pub component: Component,
    // Position mode
    #[serde(default)]
    pub position: Option<PositionMode>,
    #[serde(default)]
    pub x: Option<f32>,
    #[serde(default)]
    pub y: Option<f32>,
    /// Rendering depth: higher values render on top. Default is 0.
    #[serde(default, rename = "z-index")]
    pub z_index: Option<i32>,
    /// Overlay components positioned relative to this component's bounds.
    #[serde(default)]
    pub overlays: Vec<Overlay>,
}

/// An overlay component positioned relative to its parent.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Overlay {
    /// The component to render as overlay.
    #[serde(flatten)]
    pub component: Component,
    /// Anchor position relative to the parent.
    #[serde(default)]
    pub anchor: OverlayAnchor,
    /// Horizontal offset from the anchor position.
    #[serde(default)]
    pub offset_x: f32,
    /// Vertical offset from the anchor position.
    #[serde(default)]
    pub offset_y: f32,
}

/// Anchor position for an overlay.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema, Default)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayAnchor {
    #[default]
    TopRight,
    TopLeft,
    BottomRight,
    BottomLeft,
    Center,
}

impl ChildComponent {
    /// Returns true if this child is in flow layout (not absolute).
    pub fn is_flow(&self) -> bool {
        self.position.is_none()
    }

    /// Returns true if this child is a decorative fullscreen component (e.g. particle)
    /// that should not participate in flex flow in world scenes.
    pub fn is_decorative(&self) -> bool {
        matches!(self.component, Component::Particle(_))
    }

    /// Returns the absolute position if this child is absolutely positioned.
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

/// The main component enum — discriminated by `"type"` in JSON.
#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Component {
    // Leaf components
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
    Divider(Divider),
    Gauge(Gauge),
    Kbd(Kbd),
    Line(Line),
    Lottie(Lottie),
    Marquee(Marquee),
    Mockup(Mockup),
    Particle(Particle),
    #[serde(alias = "progress_bar")]
    Progress(Progress),
    QrCode(QrCode),
    Skeleton(Skeleton),
    Sparkline(Sparkline),
    Stat(Stat),
    RichText(RichText),
    Table(Table),
    Terminal(Terminal),
    Timeline(Timeline),
    Tooltip(Tooltip),
    // Container components
    Positioned(Positioned),
    Flex(Flex),
    Grid(Grid),
    /// Backward-compatible card container (supports both flex and grid display modes).
    Card(Card),
    /// Invisible container — groups children for shared transforms (scale, opacity, etc.).
    Container(ContainerComponent),
}

// --- Dispatch helpers ---

impl Component {
    /// Every component is a Widget.
    pub fn as_widget(&self) -> &dyn Widget {
        match self {
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
            Component::Divider(c) => c,
            Component::Gauge(c) => c,
            Component::Kbd(c) => c,
            Component::Line(c) => c,
            Component::Lottie(c) => c,
            Component::Marquee(c) => c,
            Component::Mockup(c) => c,
            Component::Particle(c) => c,
            Component::Progress(c) => c,
            Component::QrCode(c) => c,
            Component::Skeleton(c) => c,
            Component::Sparkline(c) => c,
            Component::Stat(c) => c,
            Component::RichText(c) => c,
            Component::Table(c) => c,
            Component::Terminal(c) => c,
            Component::Timeline(c) => c,
            Component::Tooltip(c) => c,
            Component::Positioned(c) => c,
            Component::Flex(c) => c,
            Component::Grid(c) => c,
            Component::Card(c) => c,
            Component::Container(c) => c,
        }
    }

    /// Returns the Animatable trait if this component supports animation.
    pub fn as_animatable(&self) -> Option<&dyn Animatable> {
        match self {
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
            Component::Divider(c) => Some(c),
            Component::Gauge(c) => Some(c),
            Component::Kbd(c) => Some(c),
            Component::Line(c) => Some(c),
            Component::Lottie(c) => Some(c),
            Component::Marquee(c) => Some(c),
            Component::Mockup(c) => Some(c),
            Component::Particle(c) => Some(c),
            Component::Progress(c) => Some(c),
            Component::QrCode(c) => Some(c),
            Component::Skeleton(c) => Some(c),
            Component::Sparkline(c) => Some(c),
            Component::Stat(c) => Some(c),
            Component::RichText(c) => Some(c),
            Component::Table(c) => Some(c),
            Component::Terminal(c) => Some(c),
            Component::Timeline(c) => Some(c),
            Component::Tooltip(c) => Some(c),
            Component::Flex(c) => Some(c),
            Component::Grid(c) => Some(c),
            Component::Card(c) => Some(c),
            Component::Container(c) => Some(c),
            Component::Positioned(_) => None,
        }
    }

    /// Returns the Timed trait if this component supports timed visibility.
    pub fn as_timed(&self) -> Option<&dyn Timed> {
        match self {
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
            Component::Divider(c) => Some(c),
            Component::Gauge(c) => Some(c),
            Component::Kbd(c) => Some(c),
            Component::Line(c) => Some(c),
            Component::Lottie(c) => Some(c),
            Component::Marquee(c) => Some(c),
            Component::Mockup(c) => Some(c),
            Component::Particle(c) => Some(c),
            Component::Progress(c) => Some(c),
            Component::QrCode(c) => Some(c),
            Component::Skeleton(c) => Some(c),
            Component::Sparkline(c) => Some(c),
            Component::Stat(c) => Some(c),
            Component::RichText(c) => Some(c),
            Component::Table(c) => Some(c),
            Component::Terminal(c) => Some(c),
            Component::Timeline(c) => Some(c),
            Component::Tooltip(c) => Some(c),
            Component::Flex(c) => Some(c),
            Component::Grid(c) => Some(c),
            Component::Card(c) => Some(c),
            Component::Container(c) => Some(c),
            Component::Caption(_) | Component::Positioned(_) => None,
        }
    }

    /// Every component supports styling (opacity, padding, margin).
    pub fn as_styled(&self) -> &dyn Styled {
        match self {
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
            Component::Divider(c) => c,
            Component::Gauge(c) => c,
            Component::Kbd(c) => c,
            Component::Line(c) => c,
            Component::Lottie(c) => c,
            Component::Marquee(c) => c,
            Component::Mockup(c) => c,
            Component::Particle(c) => c,
            Component::Progress(c) => c,
            Component::QrCode(c) => c,
            Component::Skeleton(c) => c,
            Component::Sparkline(c) => c,
            Component::Stat(c) => c,
            Component::RichText(c) => c,
            Component::Table(c) => c,
            Component::Terminal(c) => c,
            Component::Timeline(c) => c,
            Component::Tooltip(c) => c,
            Component::Positioned(c) => c,
            Component::Flex(c) => c,
            Component::Grid(c) => c,
            Component::Card(c) => c,
            Component::Container(c) => c,
        }
    }

    /// Returns the Container trait if this component has children.
    #[allow(dead_code)]
    pub fn as_container(&self) -> Option<&dyn Container> {
        match self {
            Component::Positioned(c) => Some(c),
            Component::Flex(c) => Some(c),
            Component::Grid(c) => Some(c),
            Component::Card(c) => Some(c),
            Component::Container(c) => Some(c),
            _ => None,
        }
    }

    /// Returns true if this component is a container that handles padding internally.
    pub fn is_container(&self) -> bool {
        matches!(self, Component::Positioned(_) | Component::Flex(_) | Component::Grid(_) | Component::Card(_) | Component::Container(_))
    }

}

/// Draw a gradient border on a rounded rectangle.
pub fn draw_gradient_border(
    canvas: &skia_safe::Canvas,
    rrect: &skia_safe::RRect,
    gb: &crate::schema::GradientBorder,
) {
    use skia_safe::{Paint, PaintStyle, Point, gradient_shader::GradientShaderColors};

    if gb.colors.len() < 2 {
        return;
    }

    let bounds = rrect.bounds();
    let angle_rad = gb.angle * std::f32::consts::PI / 180.0;
    let cx = bounds.center_x();
    let cy = bounds.center_y();
    let half_diag = (bounds.width().powi(2) + bounds.height().powi(2)).sqrt() / 2.0;

    let start = Point::new(
        cx - angle_rad.cos() * half_diag,
        cy - angle_rad.sin() * half_diag,
    );
    let end = Point::new(
        cx + angle_rad.cos() * half_diag,
        cy + angle_rad.sin() * half_diag,
    );

    let colors: Vec<skia_safe::Color> = gb.colors.iter().map(|c| {
        let (r, g, b, a) = crate::engine::renderer::parse_hex_color(c);
        skia_safe::Color::from_argb(a, r, g, b)
    }).collect();

    let shader = skia_safe::shader::Shader::linear_gradient(
        (start, end),
        GradientShaderColors::Colors(&colors),
        None,
        skia_safe::TileMode::Clamp,
        None,
        None,
    );

    if let Some(shader) = shader {
        let mut paint = Paint::default();
        paint.set_style(PaintStyle::Stroke);
        paint.set_stroke_width(gb.width);
        paint.set_anti_alias(true);
        paint.set_shader(shader);
        canvas.draw_rrect(rrect, &paint);
    }
}
