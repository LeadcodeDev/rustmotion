//! Bridge from the legacy `Component` tree to the new `BoxNode` tree.
//!
//! Each `ChildComponent` becomes one `BoxNode`. The component's
//! `style: LayerStyle` is converted to `CssStyle` via `legacy::layer_to_css`
//! and augmented with:
//! - `position: absolute` + `top` / `left` when `child.position` is set
//! - `width` / `height` from the component's `size` field (if any)
//! - `z-index` from the child's `z_index` field
//!
//! Container components (Card / Flex / Grid / Container / Positioned)
//! recursively build child boxes. Leaf components produce an empty-children
//! box that the dispatcher will paint.
//!
//! The builder also returns a flat `Vec<&Component>` indexed by NodeId so
//! the painter can resolve a node back to its component.

use std::sync::Arc;

use rustmotion_core::css::style::{AlignSelf, CssStyle, Position, Size as CSize};
use rustmotion_core::css::{layer_to_css, Length as CLength, LengthPercentage as CLP};
use rustmotion_core::engine::box_tree::{BoxKind, BoxNode, NodeId};
use rustmotion_core::schema::SizeDimension;

use crate::divider::DividerDirection;
use crate::flex::FlexSize;
use crate::{ChildComponent, Component};

/// Padding allowance to keep arrow/connector heads inside the box.
const ARROW_BBOX_PADDING: f32 = 16.0;

/// Result of building a box tree from a scene description.
pub struct BuiltScene<'a> {
    /// Root box (a flex column container at viewport dimensions).
    pub root: BoxNode,
    /// Lookup table — `components[id as usize]` is the component for `id`.
    /// `None` for synthetic boxes (the root scene wrapper).
    pub components: Vec<Option<&'a ChildComponent>>,
}

/// Build a box tree for a flat list of scene-level children at a given
/// viewport size.
///
/// The implicit scene root is a `display: flex; flex-direction: column;
/// width/height: 100%`; children flow vertically unless they specify
/// `position: { x, y }`, in which case they become `position: absolute`.
pub fn build_scene<'a>(
    children: &'a [ChildComponent],
    viewport: (f32, f32),
) -> BuiltScene<'a> {
    build_scene_with_root(children, viewport, default_root_css(viewport))
}

/// Same as [`build_scene`] but lets the caller supply the root container's
/// `CssStyle`. Width/height are forced to the viewport regardless.
pub fn build_scene_with_root<'a>(
    children: &'a [ChildComponent],
    viewport: (f32, f32),
    mut root_css: CssStyle,
) -> BuiltScene<'a> {
    let mut components: Vec<Option<&'a ChildComponent>> = vec![None];
    let mut next_id: NodeId = 1;

    let mut child_boxes = Vec::with_capacity(children.len());
    for c in children {
        child_boxes.push(build_child(c, &mut components, &mut next_id));
    }

    // Force the root to viewport dimensions even if the caller didn't set them.
    root_css.width = Some(CSize::Length(CLP::Px(viewport.0)));
    root_css.height = Some(CSize::Length(CLP::Px(viewport.1)));

    let root = BoxNode {
        id: 0,
        kind: BoxKind::Container,
        css: root_css,
        children: child_boxes,
        intrinsic: None,
    };

    BuiltScene { root, components }
}

fn default_root_css(viewport: (f32, f32)) -> CssStyle {
    CssStyle {
        display: Some(rustmotion_core::css::style::Display::Flex),
        flex_direction: Some(rustmotion_core::css::style::FlexDirection::Column),
        width: Some(CSize::Length(CLP::Px(viewport.0))),
        height: Some(CSize::Length(CLP::Px(viewport.1))),
        ..Default::default()
    }
}

/// Convert a single `ChildComponent` to a `BoxNode`. Recurses into containers.
fn build_child<'a>(
    child: &'a ChildComponent,
    components: &mut Vec<Option<&'a ChildComponent>>,
    next_id: &mut NodeId,
) -> BoxNode {
    let id = *next_id;
    *next_id += 1;
    components.push(Some(child));

    let mut css = component_css(&child.component);

    // Apply per-child position/z-index from the wrapper.
    if let Some((x, y)) = child.absolute_position() {
        css.position = Some(Position::Absolute);
        css.left = Some(CLP::Px(x));
        css.top = Some(CLP::Px(y));
    }
    if let Some(z) = child.z_index {
        css.z_index = Some(z);
    }

    let children_boxes = container_children(&child.component, components, next_id);
    let intrinsic = component_intrinsic(&child.component);

    BoxNode {
        id,
        kind: BoxKind::Component(Arc::new(id)),
        css,
        children: children_boxes,
        intrinsic,
    }
}

/// Build an [`IntrinsicMeasure`] for components whose box size depends on
/// their content (text, codeblock, terminal, etc.). Returns `None` for
/// components with explicit dimensions or pure containers.
fn component_intrinsic(
    component: &Component,
) -> Option<Arc<dyn rustmotion_core::engine::box_tree::IntrinsicMeasure>> {
    use Component::*;
    match component {
        Text(t) => Some(Arc::new(crate::intrinsic::TextIntrinsic::from_text(t))),
        GradientText(t) => Some(Arc::new(
            crate::intrinsic::GradientTextIntrinsic::from_gradient_text(t),
        )),
        Caption(c) => Some(Arc::new(crate::intrinsic::CaptionIntrinsic::from_caption(c))),
        Kbd(k) => Some(Arc::new(crate::intrinsic::KbdIntrinsic::from_kbd(k))),
        _ => None,
    }
}

/// If the component is a container, recurse into its children. Otherwise
/// return an empty Vec.
fn container_children<'a>(
    component: &'a Component,
    components: &mut Vec<Option<&'a ChildComponent>>,
    next_id: &mut NodeId,
) -> Vec<BoxNode> {
    let children: &[ChildComponent] = match component {
        Component::Card(c) => &c.children,
        Component::Flex(c) => &c.children,
        Component::Grid(c) => &c.children,
        Component::Container(c) => &c.children,
        Component::Positioned(c) => &c.children,
        _ => return Vec::new(),
    };
    children
        .iter()
        .map(|c| build_child(c, components, next_id))
        .collect()
}

/// Convert a component's legacy style into CSS, augmented with intrinsic
/// `width`/`height` for components that carry a fixed size.
fn component_css(component: &Component) -> CssStyle {
    let mut css = layer_to_css(component_style(component));

    if let Some((w, h)) = component_size(component) {
        if let Some(s) = size_to_css(&w) {
            css.width = Some(s);
        }
        if let Some(s) = size_to_css(&h) {
            css.height = Some(s);
        }
    }

    apply_intrinsic_overrides(component, &mut css);
    css
}

/// Apply per-component CSS overrides for things that the legacy
/// `Widget::measure` derived from constraints (e.g. divider stretching to its
/// parent, line bounding box from its endpoints).
fn apply_intrinsic_overrides(component: &Component, css: &mut CssStyle) {
    use Component::*;
    match component {
        Divider(d) => match d.direction {
            DividerDirection::Horizontal => {
                // Stretch horizontally in flex row/column parents (cross-axis
                // for column = horizontal). Width stays auto.
                if css.height.is_none() {
                    css.height = Some(CSize::Length(CLP::Px(d.thickness)));
                }
                if css.width.is_none() {
                    css.width = match d.length {
                        Some(l) => Some(CSize::Length(CLP::Px(l))),
                        None => Some(CSize::Length(CLP::String("100%".into()))),
                    };
                }
                if css.align_self.is_none() {
                    css.align_self = Some(AlignSelf::Stretch);
                }
            }
            DividerDirection::Vertical => {
                if css.width.is_none() {
                    css.width = Some(CSize::Length(CLP::Px(d.thickness)));
                }
                if css.height.is_none() {
                    css.height = match d.length {
                        Some(l) => Some(CSize::Length(CLP::Px(l))),
                        None => Some(CSize::Length(CLP::String("100%".into()))),
                    };
                }
            }
        },
        Line(l) => {
            // Line draws inside its bounding box at (x1,y1)→(x2,y2). Use the
            // bounding box as the intrinsic size so taffy reserves enough room.
            let w = (l.x2 - l.x1).abs().max(1.0);
            let h = (l.y2 - l.y1).abs().max(1.0);
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(w)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(h)));
            }
        }
        Arrow(a) => {
            // Endpoint bounding box + padding for the arrowhead/curve overshoot.
            let pad = ARROW_BBOX_PADDING + a.arrow_size.max(0.0);
            let w = (a.x2 - a.x1).abs().max(1.0) + pad;
            let h = (a.y2 - a.y1).abs().max(1.0) + pad;
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(w)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(h)));
            }
        }
        Connector(c) => {
            let pad = ARROW_BBOX_PADDING + c.arrow_size.max(0.0);
            let w = (c.to.x - c.from.x).abs().max(1.0) + pad;
            let h = (c.to.y - c.from.y).abs().max(1.0) + pad;
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(w)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(h)));
            }
        }
        _ => {}
    }
}

/// Borrow the legacy `LayerStyle` from any component.
fn component_style(c: &Component) -> &rustmotion_core::schema::LayerStyle {
    use Component::*;
    match c {
        Text(c) => &c.style,
        Shape(c) => &c.style,
        Image(c) => &c.style,
        Icon(c) => &c.style,
        Svg(c) => &c.style,
        Video(c) => &c.style,
        Gif(c) => &c.style,
        Counter(c) => &c.style,
        Cursor(c) => &c.style,
        Caption(c) => &c.style,
        Codeblock(c) => &c.style,
        Connector(c) => &c.style,
        Avatar(c) => &c.style,
        AvatarGroup(c) => &c.style,
        Arrow(c) => &c.style,
        Badge(c) => &c.style,
        Callout(c) => &c.style,
        Chart(c) => &c.style,
        Comparison(c) => &c.style,
        Countdown(c) => &c.style,
        Divider(c) => &c.style,
        DotMap(c) => &c.style,
        Gauge(c) => &c.style,
        GradientText(c) => &c.style,
        Heatmap(c) => &c.style,
        Kbd(c) => &c.style,
        Line(c) => &c.style,
        List(c) => &c.style,
        Lottie(c) => &c.style,
        Marquee(c) => &c.style,
        Mockup(c) => &c.style,
        Notification(c) => &c.style,
        Particle(c) => &c.style,
        PillNav(c) => &c.style,
        Progress(c) => &c.style,
        QrCode(c) => &c.style,
        Rating(c) => &c.style,
        Skeleton(c) => &c.style,
        Slider(c) => &c.style,
        Sparkline(c) => &c.style,
        Stat(c) => &c.style,
        Stepper(c) => &c.style,
        Switch(c) => &c.style,
        RichText(c) => &c.style,
        Table(c) => &c.style,
        TagCloud(c) => &c.style,
        Terminal(c) => &c.style,
        Timeline(c) => &c.style,
        Tooltip(c) => &c.style,
        Treemap(c) => &c.style,
        Positioned(c) => &c.style,
        Flex(c) => &c.style,
        Grid(c) => &c.style,
        Card(c) => &c.style,
        Container(c) => &c.style,
    }
}

/// Extract intrinsic width/height for components that carry a `size`.
/// Used to seed CSS `width`/`height` so taffy can lay them out without an
/// intrinsic measurer.
fn component_size(c: &Component) -> Option<(SizeDimension, SizeDimension)> {
    use Component::*;
    match c {
        Shape(c) => Some((SizeDimension::Fixed(c.size.width), SizeDimension::Fixed(c.size.height))),
        Image(c) => c.size.as_ref().map(size_pair),
        Icon(c) => c.size.as_ref().map(size_pair),
        Svg(c) => c.size.as_ref().map(size_pair),
        Video(c) => Some(size_pair(&c.size)),
        Gif(c) => c.size.as_ref().map(size_pair),
        QrCode(c) => Some((SizeDimension::Fixed(c.size), SizeDimension::Fixed(c.size))),
        Card(c) => c.size.as_ref().map(flex_size_to_dim),
        Flex(c) => c.size.as_ref().map(flex_size_to_dim),
        Grid(c) => c.size.as_ref().map(flex_size_to_dim),
        Container(c) => c.size.as_ref().map(flex_size_to_dim),
        _ => None,
    }
}

fn size_pair(s: &rustmotion_core::schema::Size) -> (SizeDimension, SizeDimension) {
    (SizeDimension::Fixed(s.width), SizeDimension::Fixed(s.height))
}

fn flex_size_to_dim(s: &FlexSize) -> (SizeDimension, SizeDimension) {
    (s.width.clone(), s.height.clone())
}

fn size_to_css(s: &SizeDimension) -> Option<CSize> {
    match s {
        SizeDimension::Fixed(v) => Some(CSize::Length(CLP::Px(*v))),
        SizeDimension::Percent(p) => Some(CSize::Length(CLP::String(format!("{}%", p)))),
        SizeDimension::Auto => Some(CSize::Auto(rustmotion_core::css::style::AutoKw::Auto)),
    }
}

// Silence unused `Length` import (kept for future intrinsic measurement work).
#[allow(dead_code)]
fn _silence(_: CLength) {}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flex::FlexSize;
    use rustmotion_core::css::style::{Display, FlexDirection};
    use rustmotion_core::engine::layout_pass::run_layout;
    use rustmotion_core::css::taffy_bridge::ConversionContext;
    use rustmotion_core::schema::{LayerStyle, Spacing};

    fn make_card(children: Vec<ChildComponent>, style: LayerStyle, size: Option<FlexSize>) -> Component {
        Component::Card(crate::card::Card {
            children,
            size,
            timing: Default::default(),
            style,
        })
    }

    fn make_shape(width: f32, height: f32) -> ChildComponent {
        ChildComponent {
            component: Component::Shape(crate::shape::Shape {
                shape: rustmotion_core::schema::ShapeType::Rect,
                size: rustmotion_core::schema::Size { width, height },
                text: None,
                timing: Default::default(),
                style: LayerStyle::default(),
            }),
            position: None,
            x: None,
            y: None,
            z_index: None,
            overlays: Vec::new(),
        }
    }

    #[test]
    fn empty_scene_has_only_root() {
        let built = build_scene(&[], (1920.0, 1080.0));
        assert_eq!(built.root.children.len(), 0);
        assert_eq!(built.components.len(), 1); // synthetic root slot
    }

    #[test]
    fn flex_card_with_two_shapes_lays_out_vertically() {
        let card = make_card(
            vec![make_shape(200.0, 50.0), make_shape(200.0, 50.0)],
            LayerStyle {
                display: Some(rustmotion_core::schema::style::CardDisplay::Flex),
                flex_direction: Some(rustmotion_core::schema::style::CardDirection::Column),
                gap: Some(10.0),
                padding: Some(Spacing::Uniform(20.0)),
                ..Default::default()
            },
            Some(FlexSize {
                width: SizeDimension::Fixed(300.0),
                height: SizeDimension::Fixed(200.0),
            }),
        );
        let scene = vec![ChildComponent {
            component: card,
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            overlays: Vec::new(),
        }];
        let built = build_scene(&scene, (1920.0, 1080.0));
        assert_eq!(built.root.children.len(), 1);
        let card_box = &built.root.children[0];
        assert_eq!(card_box.css.display, Some(Display::Flex));
        assert_eq!(card_box.css.flex_direction, Some(FlexDirection::Column));
        assert_eq!(card_box.children.len(), 2);

        let layout = run_layout(&built.root, (1920.0, 1080.0), &ConversionContext::default());
        let card_layout = layout.get(card_box.id).expect("card laid out");
        assert_eq!(card_layout.x, 0.0);
        assert_eq!(card_layout.y, 0.0);
        assert_eq!(card_layout.width, 300.0);
        assert_eq!(card_layout.height, 200.0);

        let c1 = layout.get(card_box.children[0].id).expect("shape 1 laid out");
        let c2 = layout.get(card_box.children[1].id).expect("shape 2 laid out");
        // Padding 20 from top, then first shape 50 high, gap 10 → 80.
        assert_eq!(c1.x, 20.0);
        assert_eq!(c1.y, 20.0);
        assert_eq!(c2.x, 20.0);
        assert_eq!(c2.y, 80.0);
    }

    #[test]
    fn absolute_child_uses_top_left() {
        let scene = vec![ChildComponent {
            component: Component::Shape(crate::shape::Shape {
                shape: rustmotion_core::schema::ShapeType::Rect,
                size: rustmotion_core::schema::Size { width: 100.0, height: 80.0 },
                text: None,
                timing: Default::default(),
                style: LayerStyle::default(),
            }),
            position: Some(crate::PositionMode::Absolute { x: 40.0, y: 30.0 }),
            x: None,
            y: None,
            z_index: None,
            overlays: Vec::new(),
        }];
        let built = build_scene(&scene, (400.0, 400.0));
        let layout = run_layout(&built.root, (400.0, 400.0), &ConversionContext::default());
        let shape_id = built.root.children[0].id;
        let l = layout.get(shape_id).expect("shape laid out");
        assert_eq!(l.x, 40.0);
        assert_eq!(l.y, 30.0);
        assert_eq!(l.width, 100.0);
        assert_eq!(l.height, 80.0);
    }

    #[test]
    fn horizontal_divider_stretches_to_parent_width() {
        let divider = ChildComponent {
            component: Component::Divider(crate::divider::Divider {
                direction: DividerDirection::Horizontal,
                thickness: 4.0,
                line_style: Default::default(),
                length: None,
                timing: Default::default(),
                style: LayerStyle::default(),
            }),
            position: None,
            x: None,
            y: None,
            z_index: None,
            overlays: Vec::new(),
        };
        let scene = vec![divider];
        let built = build_scene(&scene, (800.0, 600.0));
        let layout = run_layout(&built.root, (800.0, 600.0), &ConversionContext::default());
        let id = built.root.children[0].id;
        let l = layout.get(id).expect("divider laid out");
        assert_eq!(l.height, 4.0);
        assert_eq!(l.width, 800.0);
    }

    #[test]
    fn text_child_in_flex_card_gets_cosmic_intrinsic_size() {
        // A flex column card with no fixed size — its children's intrinsic
        // sizes should determine the card's width/height. The text child
        // must be measured via cosmic-text, not collapse to 0×0.
        use crate::card::Card;
        use crate::text::Text;

        let text = ChildComponent {
            component: Component::Text(Text {
                content: "Hello World".into(),
                max_width: None,
                timing: Default::default(),
                style: LayerStyle {
                    font_size: Some(40.0),
                    ..Default::default()
                },
            }),
            position: None,
            x: None,
            y: None,
            z_index: None,
            overlays: Vec::new(),
        };

        let card = ChildComponent {
            component: Component::Card(Card {
                children: vec![text],
                size: None,
                timing: Default::default(),
                style: LayerStyle {
                    display: Some(rustmotion_core::schema::style::CardDisplay::Flex),
                    flex_direction: Some(
                        rustmotion_core::schema::style::CardDirection::Column,
                    ),
                    padding: Some(Spacing::Uniform(20.0)),
                    ..Default::default()
                },
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            overlays: Vec::new(),
        };

        let scene = vec![card];
        let built = build_scene(&scene, (1920.0, 1080.0));
        let layout = run_layout(&built.root, (1920.0, 1080.0), &ConversionContext::default());

        let card_id = built.root.children[0].id;
        let text_id = built.root.children[0].children[0].id;
        let text_layout = layout.get(text_id).expect("text laid out");

        assert!(
            text_layout.width > 0.0,
            "text width should be > 0, got {}",
            text_layout.width
        );
        assert!(
            text_layout.height >= 40.0,
            "text height should be at least one line tall, got {}",
            text_layout.height
        );

        // Card height should hug the text + 2×padding(20) = ~text_h + 40.
        let card_layout = layout.get(card_id).expect("card laid out");
        assert!(
            card_layout.height >= text_layout.height + 40.0 - 1.0,
            "card height ({}) should fit text + padding ({}+40)",
            card_layout.height,
            text_layout.height,
        );
    }

    #[test]
    fn arrow_intrinsic_size_uses_endpoint_bbox_plus_arrowhead() {
        let arrow = ChildComponent {
            component: Component::Arrow(crate::arrow::Arrow {
                x1: 10.0,
                y1: 20.0,
                x2: 110.0,
                y2: 80.0,
                cp: None,
                cp1: None,
                cp2: None,
                curve: None,
                width: 4.0,
                color: "#fff".into(),
                arrow_end: true,
                arrow_start: false,
                arrow_size: 12.0,
                dashed: None,
                timing: Default::default(),
                style: LayerStyle::default(),
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            overlays: Vec::new(),
        };
        let scene = vec![arrow];
        let built = build_scene(&scene, (800.0, 600.0));
        let layout = run_layout(&built.root, (800.0, 600.0), &ConversionContext::default());
        let l = layout.get(built.root.children[0].id).expect("arrow laid out");
        // bbox 100×60 + (16 padding + 12 arrow_size) = 128×88.
        assert_eq!(l.width, 128.0);
        assert_eq!(l.height, 88.0);
    }

    #[test]
    fn connector_intrinsic_size_uses_endpoint_bbox_plus_arrowhead() {
        let conn = ChildComponent {
            component: Component::Connector(crate::connector::Connector {
                from: crate::connector::ConnectorPoint { x: 50.0, y: 0.0 },
                to: crate::connector::ConnectorPoint { x: 150.0, y: 50.0 },
                routing: Default::default(),
                curvature: 0.4,
                width: 2.0,
                color: "#fff".into(),
                arrow_end: true,
                arrow_start: false,
                arrow_size: 10.0,
                dashed: None,
                timing: Default::default(),
                style: LayerStyle::default(),
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            overlays: Vec::new(),
        };
        let scene = vec![conn];
        let built = build_scene(&scene, (800.0, 600.0));
        let layout = run_layout(&built.root, (800.0, 600.0), &ConversionContext::default());
        let l = layout.get(built.root.children[0].id).expect("connector laid out");
        // bbox 100×50 + (16 + 10) = 126×76.
        assert_eq!(l.width, 126.0);
        assert_eq!(l.height, 76.0);
    }

    #[test]
    fn line_intrinsic_size_matches_endpoint_bounding_box() {
        let line = ChildComponent {
            component: Component::Line(crate::line::Line {
                x1: 10.0,
                y1: 20.0,
                x2: 110.0,
                y2: 80.0,
                width: 2.0,
                color: "#fff".into(),
                dashed: None,
                timing: Default::default(),
                style: LayerStyle::default(),
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
            overlays: Vec::new(),
        };
        let scene = vec![line];
        let built = build_scene(&scene, (800.0, 600.0));
        let layout = run_layout(&built.root, (800.0, 600.0), &ConversionContext::default());
        let id = built.root.children[0].id;
        let l = layout.get(id).expect("line laid out");
        assert_eq!(l.width, 100.0);
        assert_eq!(l.height, 60.0);
    }
}
