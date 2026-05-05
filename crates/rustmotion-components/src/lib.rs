pub mod box_builder;
pub mod intrinsic;
pub mod legacy_dispatch;

pub mod arrow;
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
pub mod world_bitmap;
pub mod flex;
pub mod gauge;
pub mod gif;
pub mod gradient_text;
pub mod grid;
pub mod heatmap;
pub mod icon;
pub mod kbd;
pub mod line;
pub mod list;
pub mod lottie;
pub mod marquee;
pub mod image;
pub mod mockup;
pub mod notification;
pub mod particle;
pub mod pill_nav;
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

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use rustmotion_core::traits::{Animatable, Container, Painter, Styled, Timed, Widget};

pub use arrow::Arrow;
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
pub use particle::Particle;
pub use pill_nav::PillNav;
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

// --- Position mode ---

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PositionMode {
    Absolute { x: f32, y: f32 },
    Named(String),
}

impl Default for PositionMode {
    fn default() -> Self {
        Self::Absolute { x: 0.0, y: 0.0 }
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
    #[serde(default)]
    pub overlays: Vec<Overlay>,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
pub struct Overlay {
    #[serde(flatten)]
    pub component: Component,
    #[serde(default)]
    pub anchor: OverlayAnchor,
    #[serde(default)]
    pub offset_x: f32,
    #[serde(default)]
    pub offset_y: f32,
}

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

impl rustmotion_core::traits::LayoutItem for ChildComponent {
    fn as_widget(&self) -> &dyn Widget {
        self.component.as_widget()
    }
    fn as_styled(&self) -> &dyn rustmotion_core::traits::Styled {
        self.component.as_styled()
    }
    fn is_container(&self) -> bool {
        self.component.is_container()
    }
    fn is_flow(&self) -> bool {
        self.position.is_none()
    }
    fn is_decorative(&self) -> bool {
        matches!(self.component, Component::Particle(_))
    }
    fn absolute_position(&self) -> Option<(f32, f32)> {
        ChildComponent::absolute_position(self)
    }
    fn default_position(&self) -> (f32, f32) {
        (self.x.unwrap_or(0.0), self.y.unwrap_or(0.0))
    }
}

// --- Component enum ---

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Component {
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
    Container(ContainerComponent),
}

// --- Dispatch helpers ---

impl Component {
    pub fn as_widget(&self) -> &dyn Widget {
        match self {
            Component::Text(c) => c, Component::Shape(c) => c, Component::Image(c) => c,
            Component::Icon(c) => c, Component::Svg(c) => c, Component::Video(c) => c,
            Component::Gif(c) => c, Component::Counter(c) => c, Component::Cursor(c) => c,
            Component::Caption(c) => c, Component::Codeblock(c) => c, Component::Avatar(c) => c,
            Component::AvatarGroup(c) => c, Component::Arrow(c) => c, Component::Connector(c) => c,
            Component::Badge(c) => c, Component::Callout(c) => c, Component::Chart(c) => c,
            Component::Comparison(c) => c, Component::Countdown(c) => c,
            Component::Divider(c) => c, Component::DotMap(c) => c, Component::Gauge(c) => c,
            Component::GradientText(c) => c, Component::Heatmap(c) => c,
            Component::Kbd(c) => c, Component::Line(c) => c, Component::List(c) => c,
            Component::Lottie(c) => c, Component::Marquee(c) => c, Component::Mockup(c) => c,
            Component::Notification(c) => c, Component::Particle(c) => c,
            Component::PillNav(c) => c, Component::Progress(c) => c, Component::QrCode(c) => c,
            Component::Rating(c) => c, Component::Skeleton(c) => c, Component::Slider(c) => c,
            Component::Sparkline(c) => c, Component::Stat(c) => c, Component::Stepper(c) => c,
            Component::Switch(c) => c, Component::RichText(c) => c, Component::Table(c) => c,
            Component::TagCloud(c) => c, Component::Terminal(c) => c, Component::Timeline(c) => c,
            Component::Tooltip(c) => c, Component::Treemap(c) => c,
            Component::Positioned(c) => c, Component::Flex(c) => c, Component::Grid(c) => c,
            Component::Card(c) => c, Component::Container(c) => c,
        }
    }

    pub fn as_animatable(&self) -> Option<&dyn Animatable> {
        match self {
            Component::Text(c) => Some(c), Component::Shape(c) => Some(c), Component::Image(c) => Some(c),
            Component::Icon(c) => Some(c), Component::Svg(c) => Some(c), Component::Video(c) => Some(c),
            Component::Gif(c) => Some(c), Component::Counter(c) => Some(c), Component::Cursor(c) => Some(c),
            Component::Caption(c) => Some(c), Component::Codeblock(c) => Some(c), Component::Avatar(c) => Some(c),
            Component::AvatarGroup(c) => Some(c), Component::Arrow(c) => Some(c), Component::Connector(c) => Some(c),
            Component::Badge(c) => Some(c), Component::Callout(c) => Some(c), Component::Chart(c) => Some(c),
            Component::Comparison(c) => Some(c), Component::Countdown(c) => Some(c),
            Component::Divider(c) => Some(c), Component::DotMap(c) => Some(c), Component::Gauge(c) => Some(c),
            Component::GradientText(c) => Some(c), Component::Heatmap(c) => Some(c),
            Component::Kbd(c) => Some(c), Component::Line(c) => Some(c), Component::List(c) => Some(c),
            Component::Lottie(c) => Some(c), Component::Marquee(c) => Some(c), Component::Mockup(c) => Some(c),
            Component::Notification(c) => Some(c), Component::Particle(c) => Some(c),
            Component::PillNav(c) => Some(c), Component::Progress(c) => Some(c), Component::QrCode(c) => Some(c),
            Component::Rating(c) => Some(c), Component::Skeleton(c) => Some(c), Component::Slider(c) => Some(c),
            Component::Sparkline(c) => Some(c), Component::Stat(c) => Some(c), Component::Stepper(c) => Some(c),
            Component::Switch(c) => Some(c), Component::RichText(c) => Some(c), Component::Table(c) => Some(c),
            Component::TagCloud(c) => Some(c), Component::Terminal(c) => Some(c), Component::Timeline(c) => Some(c),
            Component::Tooltip(c) => Some(c), Component::Treemap(c) => Some(c),
            Component::Flex(c) => Some(c), Component::Grid(c) => Some(c),
            Component::Card(c) => Some(c), Component::Container(c) => Some(c),
            Component::Positioned(_) => None,
        }
    }

    pub fn as_timed(&self) -> Option<&dyn Timed> {
        match self {
            Component::Text(c) => Some(c), Component::Shape(c) => Some(c), Component::Image(c) => Some(c),
            Component::Icon(c) => Some(c), Component::Svg(c) => Some(c), Component::Video(c) => Some(c),
            Component::Gif(c) => Some(c), Component::Counter(c) => Some(c), Component::Cursor(c) => Some(c),
            Component::Codeblock(c) => Some(c), Component::Avatar(c) => Some(c),
            Component::AvatarGroup(c) => Some(c), Component::Arrow(c) => Some(c), Component::Connector(c) => Some(c),
            Component::Badge(c) => Some(c), Component::Callout(c) => Some(c), Component::Chart(c) => Some(c),
            Component::Comparison(c) => Some(c), Component::Countdown(c) => Some(c),
            Component::Divider(c) => Some(c), Component::DotMap(c) => Some(c), Component::Gauge(c) => Some(c),
            Component::GradientText(c) => Some(c), Component::Heatmap(c) => Some(c),
            Component::Kbd(c) => Some(c), Component::Line(c) => Some(c), Component::List(c) => Some(c),
            Component::Lottie(c) => Some(c), Component::Marquee(c) => Some(c), Component::Mockup(c) => Some(c),
            Component::Notification(c) => Some(c), Component::Particle(c) => Some(c),
            Component::PillNav(c) => Some(c), Component::Progress(c) => Some(c), Component::QrCode(c) => Some(c),
            Component::Rating(c) => Some(c), Component::Skeleton(c) => Some(c), Component::Slider(c) => Some(c),
            Component::Sparkline(c) => Some(c), Component::Stat(c) => Some(c), Component::Stepper(c) => Some(c),
            Component::Switch(c) => Some(c), Component::RichText(c) => Some(c), Component::Table(c) => Some(c),
            Component::TagCloud(c) => Some(c), Component::Terminal(c) => Some(c), Component::Timeline(c) => Some(c),
            Component::Tooltip(c) => Some(c), Component::Treemap(c) => Some(c),
            Component::Flex(c) => Some(c), Component::Grid(c) => Some(c),
            Component::Card(c) => Some(c), Component::Container(c) => Some(c),
            Component::Caption(_) | Component::Positioned(_) => None,
        }
    }

    pub fn as_styled(&self) -> &dyn Styled {
        match self {
            Component::Text(c) => c, Component::Shape(c) => c, Component::Image(c) => c,
            Component::Icon(c) => c, Component::Svg(c) => c, Component::Video(c) => c,
            Component::Gif(c) => c, Component::Counter(c) => c, Component::Cursor(c) => c,
            Component::Caption(c) => c, Component::Codeblock(c) => c, Component::Avatar(c) => c,
            Component::AvatarGroup(c) => c, Component::Arrow(c) => c, Component::Connector(c) => c,
            Component::Badge(c) => c, Component::Callout(c) => c, Component::Chart(c) => c,
            Component::Comparison(c) => c, Component::Countdown(c) => c,
            Component::Divider(c) => c, Component::DotMap(c) => c, Component::Gauge(c) => c,
            Component::GradientText(c) => c, Component::Heatmap(c) => c,
            Component::Kbd(c) => c, Component::Line(c) => c, Component::List(c) => c,
            Component::Lottie(c) => c, Component::Marquee(c) => c, Component::Mockup(c) => c,
            Component::Notification(c) => c, Component::Particle(c) => c,
            Component::PillNav(c) => c, Component::Progress(c) => c, Component::QrCode(c) => c,
            Component::Rating(c) => c, Component::Skeleton(c) => c, Component::Slider(c) => c,
            Component::Sparkline(c) => c, Component::Stat(c) => c, Component::Stepper(c) => c,
            Component::Switch(c) => c, Component::RichText(c) => c, Component::Table(c) => c,
            Component::TagCloud(c) => c, Component::Terminal(c) => c, Component::Timeline(c) => c,
            Component::Tooltip(c) => c, Component::Treemap(c) => c,
            Component::Positioned(c) => c, Component::Flex(c) => c, Component::Grid(c) => c,
            Component::Card(c) => c, Component::Container(c) => c,
        }
    }

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

    /// Returns the Painter trait. All 51 components are migrated to the new
    /// pipeline; the dispatcher always uses Painter::paint_content.
    pub fn as_painter(&self) -> Option<&dyn Painter> {
        match self {
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

    pub fn is_container(&self) -> bool {
        matches!(self, Component::Positioned(_) | Component::Flex(_) | Component::Grid(_) | Component::Card(_) | Component::Container(_))
    }
}

/// Draw a gradient border on a rounded rectangle.
pub fn draw_gradient_border(
    canvas: &skia_safe::Canvas,
    rrect: &skia_safe::RRect,
    gb: &rustmotion_core::schema::GradientBorder,
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
        let (r, g, b, a) = rustmotion_core::engine::renderer::parse_hex_color(c);
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
