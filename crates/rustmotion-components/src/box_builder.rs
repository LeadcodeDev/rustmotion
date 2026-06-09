//! Bridge from the `Component` tree to the new `BoxNode` tree.
//!
//! Each `ChildComponent` becomes one `BoxNode`. The component's
//! `style: CssStyle` is augmented with:
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
use rustmotion_core::css::{apply_animated_props, LengthPercentage as CLP};
use rustmotion_core::engine::animator::{resolve_props_for_effects, AnimatedProperties};
use rustmotion_core::engine::box_tree::{BoxKind, BoxNode, NodeId};

use crate::divider::DividerDirection;
use crate::timeline::TimelineDirection;
use crate::{ChildComponent, Component};

/// Frame-level context passed into the builder so animations can be resolved
/// per-node and merged into the resulting `CssStyle`. When `None`, the box
/// tree is built without any animation overrides (resting state).
#[derive(Debug, Clone, Copy)]
pub struct BuildAnimationCtx {
    pub time: f64,
    pub scene_duration: f64,
}

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
    root_css: CssStyle,
) -> BuiltScene<'a> {
    build_scene_from_refs(children.iter(), viewport, root_css, None)
}

/// Like [`build_scene_with_root`] but resolves animations at `time` (seconds)
/// for each node and merges the result into its `CssStyle`. Use this when
/// rendering an animated frame.
pub fn build_scene_at_time<'a>(
    children: &'a [ChildComponent],
    viewport: (f32, f32),
    root_css: CssStyle,
    anim: BuildAnimationCtx,
) -> BuiltScene<'a> {
    build_scene_from_refs(children.iter(), viewport, root_css, Some(anim))
}

/// Like [`build_scene`] but with an animation context. Convenience wrapper
/// that uses the default root CSS (full-viewport flex column).
pub fn build_scene_with_anim<'a>(
    children: &'a [ChildComponent],
    viewport: (f32, f32),
    anim: BuildAnimationCtx,
) -> BuiltScene<'a> {
    build_scene_from_refs(children.iter(), viewport, default_root_css(viewport), Some(anim))
}

/// Same as [`build_scene_with_root`] but accepts an iterator over
/// `&ChildComponent` references. Useful when the caller has filtered or
/// re-ordered the scene's children and doesn't want to clone.
pub fn build_scene_from_refs<'a, I>(
    children: I,
    viewport: (f32, f32),
    mut root_css: CssStyle,
    anim: Option<BuildAnimationCtx>,
) -> BuiltScene<'a>
where
    I: IntoIterator<Item = &'a ChildComponent>,
{
    let mut components: Vec<Option<&'a ChildComponent>> = vec![None];
    let mut next_id: NodeId = 1;

    let mut child_boxes = Vec::new();
    for (i, c) in children.into_iter().enumerate() {
        child_boxes.push(build_child(
            c,
            &mut components,
            &mut next_id,
            anim,
            format!("/children/{i}"),
        ));
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
        source_path: None,
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
    anim: Option<BuildAnimationCtx>,
    path: String,
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

    // Resolve animations and apply transform/opacity/filter overrides on the
    // box's CSS. Only paint-time properties (transform, opacity, filter,
    // perspective) flow into CSS — internal animations like draw_progress or
    // char_animation remain on the `AnimatedProperties` legacy path.
    if let Some(actx) = anim {
        if let Some(animatable) = child.component.as_animatable() {
            let effects = animatable.animation_effects();
            if !effects.is_empty() {
                let props = resolve_props_for_effects(effects, actx.time, actx.scene_duration);
                if props_has_paint_overrides(&props) {
                    apply_animated_props(&mut css, &props);
                }
            }
        }
    }

    let children_boxes = container_children(&child.component, components, next_id, anim, &path);
    let intrinsic = component_intrinsic(&child.component);

    BoxNode {
        id,
        kind: BoxKind::Component(Arc::new(id)),
        css,
        children: children_boxes,
        intrinsic,
        source_path: Some(path),
    }
}

/// Quick gate: does this resolved `AnimatedProperties` carry any property
/// that we know how to translate to CSS? Avoids allocating a transform Vec
/// when there's nothing to apply.
fn props_has_paint_overrides(p: &AnimatedProperties) -> bool {
    p.translate_x != 0.0
        || p.translate_y != 0.0
        || (p.scale_x - 1.0).abs() > 1e-4
        || (p.scale_y - 1.0).abs() > 1e-4
        || p.rotation.abs() > 1e-3
        || p.rotate_x.abs() > 1e-3
        || p.rotate_y.abs() > 1e-3
        || (p.opacity - 1.0).abs() > 1e-4
        || p.blur > 0.0
        || (p.glow_radius > 0.0 && p.glow_intensity > 0.0)
        || p.perspective > 0.0
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
        Counter(c) => Some(Arc::new(crate::intrinsic::CounterIntrinsic::from_counter(c))),
        Badge(b) => Some(Arc::new(crate::intrinsic::BadgeIntrinsic::from_badge(b))),
        _ => None,
    }
}

/// If the component is a container, recurse into its children. Otherwise
/// return an empty Vec.
fn container_children<'a>(
    component: &'a Component,
    components: &mut Vec<Option<&'a ChildComponent>>,
    next_id: &mut NodeId,
    anim: Option<BuildAnimationCtx>,
    parent_path: &str,
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
        .enumerate()
        .map(|(j, c)| build_child(c, components, next_id, anim, format!("{parent_path}/children/{j}")))
        .collect()
}

/// Pull the component's `CssStyle`, augmented with intrinsic `width`/`height`
/// for components that carry a fixed size.
fn component_css(component: &Component) -> CssStyle {
    let mut css = component_style(component).clone();
    apply_default_display(component, &mut css);
    apply_intrinsic_overrides(component, &mut css);
    css
}

/// Set `display` from the component kind when the user didn't specify one.
/// `card` / `flex` → `flex`, `grid` → `grid`. The taffy bridge defaults to
/// `block` otherwise, which would silently ignore `flex-direction` & friends.
fn apply_default_display(component: &Component, css: &mut CssStyle) {
    use rustmotion_core::css::style::Display;
    if css.display.is_some() {
        return;
    }
    css.display = match component {
        Component::Card(_) | Component::Flex(_) | Component::Container(_) => Some(Display::Flex),
        Component::Grid(_) => Some(Display::Grid),
        _ => return,
    };
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
        Cursor(cur) => {
            // Fixed-size pointer; legacy measure returns (width, height).
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(cur.width)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(cur.height)));
            }
        }
        Particle(_) => {
            // Particles fill their parent (legacy returned the max constraints).
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::String("100%".into())));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::String("100%".into())));
            }
        }
        Switch(c) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(c.width)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(c.height)));
            }
        }
        Slider(c) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(c.width)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(c.height)));
            }
        }
        Progress(c) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(c.width)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(c.height)));
            }
        }
        List(c) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(c.width)));
            }
            if css.height.is_none() {
                let font_size = c.style.font_size_px_or(16.0);
                let line_height = font_size * 1.3;
                let n = c.items.len() as f32;
                let h = n * line_height + (n - 1.0).max(0.0) * c.gap;
                css.height = Some(CSize::Length(CLP::Px(h)));
            }
        }
        Timeline(c) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(c.width)));
            }
            if css.height.is_none() {
                let r = c.node_radius;
                let h = match c.direction {
                    TimelineDirection::Horizontal => r * 2.0 + c.font_size * 2.5 + 24.0,
                    TimelineDirection::Vertical => {
                        let n = c.steps.len().max(1) as f32;
                        n * (r * 2.0 + 64.0)
                    }
                };
                css.height = Some(CSize::Length(CLP::Px(h)));
            }
        }
        Notification(c) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(c.width)));
            }
            if css.height.is_none() {
                let h = if c.message.is_some() { 96.0 } else { 64.0 };
                css.height = Some(CSize::Length(CLP::Px(h)));
            }
        }
        Rating(c) => {
            if css.width.is_none() {
                let count = c.max as f32;
                let w = count * c.size + (count - 1.0).max(0.0) * c.gap;
                css.width = Some(CSize::Length(CLP::Px(w)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(c.size)));
            }
        }
        Avatar(c) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(c.size)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(c.size)));
            }
        }
        AvatarGroup(c) => {
            if css.width.is_none() {
                let visible = c.visible_count() as f32;
                let extra = if c.overflow_count() > 0 { 1.0 } else { 0.0 };
                let total = visible + extra;
                let step = (c.size - c.overlap).max(0.0);
                let w = if total <= 0.0 { 0.0 } else { c.size + (total - 1.0) * step };
                css.width = Some(CSize::Length(CLP::Px(w)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(c.size)));
            }
        }
        QrCode(c) => {
            if css.width.is_none() {
                css.width = Some(CSize::Length(CLP::Px(c.size)));
            }
            if css.height.is_none() {
                css.height = Some(CSize::Length(CLP::Px(c.size)));
            }
        }
        Countdown(c) => {
            if css.width.is_none() || css.height.is_none() {
                let visible = [c.show_hours, c.show_minutes, c.show_seconds]
                    .iter()
                    .filter(|v| **v)
                    .count() as f32;
                let box_w = c.digit_size * 0.75;
                let box_h = c.digit_size * 1.2;
                let w = (visible * 2.0 * box_w) + ((visible - 1.0).max(0.0) * c.gap);
                if css.width.is_none() {
                    css.width = Some(CSize::Length(CLP::Px(w)));
                }
                if css.height.is_none() {
                    css.height = Some(CSize::Length(CLP::Px(box_h)));
                }
            }
        }
        _ => {}
    }
}

/// Borrow the `CssStyle` from any component.
fn component_style(c: &Component) -> &CssStyle {
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

/// Short kind label for a component (for studio selection / inspector display).
pub fn component_kind(c: &Component) -> &'static str {
    use Component::*;
    match c {
        Text(_) => "text",
        Shape(_) => "shape",
        Image(_) => "image",
        Icon(_) => "icon",
        Svg(_) => "svg",
        Video(_) => "video",
        Gif(_) => "gif",
        Counter(_) => "counter",
        Cursor(_) => "cursor",
        Caption(_) => "caption",
        Codeblock(_) => "codeblock",
        Connector(_) => "connector",
        Avatar(_) => "avatar",
        AvatarGroup(_) => "avatar_group",
        Arrow(_) => "arrow",
        Badge(_) => "badge",
        Callout(_) => "callout",
        Chart(_) => "chart",
        Comparison(_) => "comparison",
        Countdown(_) => "countdown",
        Divider(_) => "divider",
        DotMap(_) => "dot_map",
        Gauge(_) => "gauge",
        GradientText(_) => "gradient_text",
        Heatmap(_) => "heatmap",
        Kbd(_) => "kbd",
        Line(_) => "line",
        List(_) => "list",
        Lottie(_) => "lottie",
        Marquee(_) => "marquee",
        Mockup(_) => "mockup",
        Notification(_) => "notification",
        Particle(_) => "particle",
        PillNav(_) => "pill_nav",
        Progress(_) => "progress",
        QrCode(_) => "qrcode",
        Rating(_) => "rating",
        Skeleton(_) => "skeleton",
        Slider(_) => "slider",
        Sparkline(_) => "sparkline",
        Stat(_) => "stat",
        Stepper(_) => "stepper",
        Switch(_) => "switch",
        RichText(_) => "rich_text",
        Table(_) => "table",
        TagCloud(_) => "tag_cloud",
        Terminal(_) => "terminal",
        Timeline(_) => "timeline",
        Tooltip(_) => "tooltip",
        Treemap(_) => "treemap",
        Positioned(_) => "positioned",
        Flex(_) => "flex",
        Grid(_) => "grid",
        Card(_) => "card",
        Container(_) => "container",
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use rustmotion_core::css::style::{CssStyle, Display, Edges, FlexDirection, Gap, Size as CSize};
    use rustmotion_core::css::units::LengthPercentage;
    use rustmotion_core::css::units::LengthPercentage as CLP;
    use rustmotion_core::engine::layout_pass::run_layout;
    use rustmotion_core::css::taffy_bridge::ConversionContext;

    fn make_card(children: Vec<ChildComponent>, style: CssStyle) -> Component {
        Component::Card(crate::card::Card {
            children,
            timing: Default::default(),
            style,
            timeline: Vec::new(),
            stagger: None,
        })
    }

    fn make_shape(width: f32, height: f32) -> ChildComponent {
        ChildComponent {
            component: Component::Shape(crate::shape::Shape {
                shape: rustmotion_core::schema::ShapeType::Rect,
                text: None,
                timing: Default::default(),
                style: CssStyle {
                    width: Some(CSize::Length(CLP::Px(width))),
                    height: Some(CSize::Length(CLP::Px(height))),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
                fill: None,
                stroke: None,
            }),
            position: None,
            x: None,
            y: None,
            z_index: None,
        }
    }

    #[test]
    fn empty_scene_has_only_root() {
        let built = build_scene(&[], (1920.0, 1080.0));
        assert_eq!(built.root.children.len(), 0);
        assert_eq!(built.components.len(), 1); // synthetic root slot
    }

    #[test]
    fn component_kind_labels() {
        assert_eq!(component_kind(&make_shape(100.0, 50.0).component), "shape");
    }

    #[test]
    fn build_child_records_source_path() {
        let card = make_card(
            vec![make_shape(10.0, 10.0), make_shape(10.0, 10.0)],
            CssStyle::default(),
        );
        let scene = vec![ChildComponent {
            component: card,
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
        }];
        let built = build_scene(&scene, (800.0, 600.0));
        let card_box = &built.root.children[0];
        assert_eq!(card_box.source_path.as_deref(), Some("/children/0"));
        assert_eq!(
            card_box.children[1].source_path.as_deref(),
            Some("/children/0/children/1")
        );
    }

    #[test]
    fn flex_card_with_two_shapes_lays_out_vertically() {
        let card = make_card(
            vec![make_shape(200.0, 50.0), make_shape(200.0, 50.0)],
            CssStyle {
                display: Some(Display::Flex),
                flex_direction: Some(FlexDirection::Column),
                gap: Some(Gap::Uniform(LengthPercentage::Px(10.0))),
                padding: Some(Edges::Uniform(LengthPercentage::Px(20.0))),
                width: Some(CSize::Length(CLP::Px(300.0))),
                height: Some(CSize::Length(CLP::Px(200.0))),
                ..Default::default()
            },
        );
        let scene = vec![ChildComponent {
            component: card,
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
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
                text: None,
                timing: Default::default(),
                style: CssStyle {
                    width: Some(CSize::Length(CLP::Px(100.0))),
                    height: Some(CSize::Length(CLP::Px(80.0))),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
                fill: None,
                stroke: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 40.0, y: 30.0 }),
            x: None,
            y: None,
            z_index: None,
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
                style: CssStyle::default(),
                timeline: Vec::new(),
                stagger: None,
            }),
            position: None,
            x: None,
            y: None,
            z_index: None,
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

        use rustmotion_core::css::units::Length;

        let text = ChildComponent {
            component: Component::Text(Text {
                content: "Hello World".into(),
                max_width: None,
                timing: Default::default(),
                style: CssStyle {
                    font_size: Some(Length::Px(40.0)),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
                text_shadow: None,
                stroke: None,
                text_background: None,
            }),
            position: None,
            x: None,
            y: None,
            z_index: None,
        };

        let card = ChildComponent {
            component: Component::Card(Card {
                children: vec![text],
                timing: Default::default(),
                style: CssStyle {
                    display: Some(Display::Flex),
                    flex_direction: Some(FlexDirection::Column),
                    padding: Some(Edges::Uniform(LengthPercentage::Px(20.0))),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
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
                style: CssStyle::default(),
                timeline: Vec::new(),
                stagger: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
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
                style: CssStyle::default(),
                timeline: Vec::new(),
                stagger: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
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
    fn counter_intrinsic_size_reserves_space_for_max_value() {
        // 1234 → 1234 → format with 0 decimals → measure largest absolute value.
        // Expectation: width > 0 (cosmic-text didn't fail), height ≈ font_size × line_height.
        use crate::counter::Counter;

        use rustmotion_core::css::units::Length;
        let counter = ChildComponent {
            component: Component::Counter(Counter {
                from: 0.0,
                to: 1234.0,
                decimals: 0,
                separator: None,
                prefix: None,
                suffix: None,
                easing: Default::default(),
                timing: Default::default(),
                style: CssStyle {
                    font_size: Some(Length::Px(64.0)),
                    ..Default::default()
                },
                timeline: Vec::new(),
                stagger: None,
                text_shadow: None,
                stroke: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 10.0, y: 20.0 }),
            x: None,
            y: None,
            z_index: None,
        };
        let scene = vec![counter];
        let built = build_scene(&scene, (800.0, 600.0));
        let layout = run_layout(&built.root, (800.0, 600.0), &ConversionContext::default());
        let id = built.root.children[0].id;
        let l = layout.get(id).expect("counter laid out");
        assert!(l.width > 0.0, "counter width should be > 0, got {}", l.width);
        // line_height defaults to font_size × 1.3 = 83.2. Allow some slack.
        assert!(
            l.height >= 60.0,
            "counter height should be ≥ ~one line ({}), got {}",
            64.0,
            l.height
        );
    }

    #[test]
    fn badge_intrinsic_size_includes_padding_and_text() {
        // Default size = Md → font_size 14, h_pad 12, v_pad 6, icon 18.
        // Without an icon, height ≈ 6×2 + 14×1.3 ≈ 30.2.
        use crate::badge::{Badge, BadgeSize, BadgeVariant};

        let badge = ChildComponent {
            component: Component::Badge(Badge {
                text: "New".into(),
                icon: None,
                variant: BadgeVariant::Solid,
                badge_size: BadgeSize::Md,
                dot: false,
                dot_color: None,
                pulse: false,
                count: None,
                timing: Default::default(),
                style: CssStyle::default(),
                timeline: Vec::new(),
                stagger: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
        };
        let scene = vec![badge];
        let built = build_scene(&scene, (400.0, 200.0));
        let layout = run_layout(&built.root, (400.0, 200.0), &ConversionContext::default());
        let id = built.root.children[0].id;
        let l = layout.get(id).expect("badge laid out");
        // h_pad×2 = 24 alone, plus the text width.
        assert!(l.width > 24.0, "badge width should exceed padding alone, got {}", l.width);
        assert!(
            (l.height - 30.2).abs() < 2.0,
            "badge height should be ~30.2, got {}",
            l.height
        );
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
                style: CssStyle::default(),
                timeline: Vec::new(),
                stagger: None,
            }),
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
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
