pub mod caption;
pub mod card;
pub mod codeblock;
pub mod counter;
pub mod flex;
pub mod gif;
pub mod grid;
pub mod icon;
pub mod image;
pub mod shape;
pub mod stack;
pub mod svg;
pub mod text;
pub mod video;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::traits::{Animatable, Container, Styled, Timed, Widget};

pub use caption::Caption;
pub use card::Card;
pub use codeblock::Codeblock;
pub use counter::Counter;
pub use flex::Flex;
pub use gif::Gif;
pub use grid::Grid;
pub use icon::Icon;
pub use image::Image;
pub use shape::Shape;
pub use stack::Stack;
pub use svg::Svg;
pub use text::Text;
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
}

impl ChildComponent {
    /// Returns true if this child is in flow layout (not absolute).
    pub fn is_flow(&self) -> bool {
        self.position.is_none()
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
    Caption(Caption),
    Codeblock(Codeblock),
    // Container components
    Stack(Stack),
    Flex(Flex),
    Grid(Grid),
    /// Backward-compatible card container (supports both flex and grid display modes).
    Card(Card),
    /// Backward-compatible group container (alias for Stack with absolute positioning).
    Group(Stack),
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
            Component::Caption(c) => c,
            Component::Codeblock(c) => c,
            Component::Stack(c) | Component::Group(c) => c,
            Component::Flex(c) => c,
            Component::Grid(c) => c,
            Component::Card(c) => c,
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
            Component::Caption(c) => Some(c),
            Component::Codeblock(c) => Some(c),
            Component::Flex(c) => Some(c),
            Component::Grid(c) => Some(c),
            Component::Card(c) => Some(c),
            // Stack/Group has no animation support
            Component::Stack(_) | Component::Group(_) => None,
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
            Component::Codeblock(c) => Some(c),
            Component::Flex(c) => Some(c),
            Component::Grid(c) => Some(c),
            Component::Card(c) => Some(c),
            // Caption and Stack/Group have no timed visibility
            Component::Caption(_) | Component::Stack(_) | Component::Group(_) => None,
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
            Component::Caption(c) => c,
            Component::Codeblock(c) => c,
            Component::Stack(c) | Component::Group(c) => c,
            Component::Flex(c) => c,
            Component::Grid(c) => c,
            Component::Card(c) => c,
        }
    }

    /// Returns the Container trait if this component has children.
    #[allow(dead_code)]
    pub fn as_container(&self) -> Option<&dyn Container> {
        match self {
            Component::Stack(c) | Component::Group(c) => Some(c),
            Component::Flex(c) => Some(c),
            Component::Grid(c) => Some(c),
            Component::Card(c) => Some(c),
            _ => None,
        }
    }

    /// Returns a mutable reference to this component's children, if it has any.
    pub fn children_mut(&mut self) -> Option<&mut Vec<ChildComponent>> {
        match self {
            Component::Stack(c) | Component::Group(c) => Some(&mut c.children),
            Component::Flex(c) => Some(&mut c.children),
            Component::Grid(c) => Some(&mut c.children),
            Component::Card(c) => Some(&mut c.children),
            _ => None,
        }
    }
}
