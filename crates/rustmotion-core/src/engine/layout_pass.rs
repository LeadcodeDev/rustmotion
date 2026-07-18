//! Layout pass — runs taffy on the BoxTree to produce per-node geometry.

use std::collections::HashMap;

use taffy::prelude as tf;
use taffy::TaffyTree;

use crate::css::taffy_bridge::{to_taffy_style, ConversionContext};
use crate::engine::box_tree::{BoxNode, IntrinsicMeasure, NodeId};

/// Resolved geometry for a single node, in absolute viewport coordinates.
#[derive(Debug, Clone, Copy, Default)]
pub struct BoxLayout {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub border: Insets,
    pub padding: Insets,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

impl BoxLayout {
    pub fn content_box(&self) -> (f32, f32, f32, f32) {
        let x = self.x + self.border.left + self.padding.left;
        let y = self.y + self.border.top + self.padding.top;
        let w = (self.width
            - self.border.left
            - self.border.right
            - self.padding.left
            - self.padding.right)
            .max(0.0);
        let h = (self.height
            - self.border.top
            - self.border.bottom
            - self.padding.top
            - self.padding.bottom)
            .max(0.0);
        (x, y, w, h)
    }

    pub fn padding_box(&self) -> (f32, f32, f32, f32) {
        let x = self.x + self.border.left;
        let y = self.y + self.border.top;
        let w = (self.width - self.border.left - self.border.right).max(0.0);
        let h = (self.height - self.border.top - self.border.bottom).max(0.0);
        (x, y, w, h)
    }
}

/// Result of a layout pass: layout per node, indexed by `NodeId`.
#[derive(Debug, Clone, Default)]
pub struct LayoutResult {
    pub layouts: HashMap<NodeId, BoxLayout>,
}

impl LayoutResult {
    pub fn get(&self, id: NodeId) -> Option<&BoxLayout> {
        self.layouts.get(&id)
    }
}

/// Per-node user data stored in the taffy tree to keep the link between
/// taffy nodes and our `BoxNode` ids + intrinsic measurers.
struct NodeData {
    #[allow(dead_code)]
    box_id: NodeId,
    intrinsic: Option<std::sync::Arc<dyn IntrinsicMeasure>>,
}

/// Run taffy on a [`BoxNode`] tree and return the resolved layouts.
pub fn run_layout(root: &BoxNode, viewport: (f32, f32), ctx: &ConversionContext) -> LayoutResult {
    let mut tree: TaffyTree<NodeData> = TaffyTree::new();
    // Disable taffy's pixel rounding: it floors widths/heights to integers,
    // but our painters use sub-pixel Skia metrics. A width of 710.376 rounded
    // to 710 makes the painter re-wrap onto an extra line.
    tree.disable_rounding();
    let mut node_map: HashMap<NodeId, tf::NodeId> = HashMap::new();

    // Build the taffy tree top-down.
    let root_tf = build(&mut tree, &mut node_map, root, ctx);

    let viewport_size = tf::Size {
        width: tf::AvailableSpace::Definite(viewport.0),
        height: tf::AvailableSpace::Definite(viewport.1),
    };
    let _ = tree.compute_layout_with_measure(
        root_tf,
        viewport_size,
        |known, available, _node, ctx_data, _style| {
            let Some(ctx) = ctx_data else {
                return tf::Size::ZERO;
            };
            let Some(intr) = ctx.intrinsic.as_ref() else {
                return tf::Size::ZERO;
            };
            let (w, h) = intr.measure(
                (known.width, known.height),
                (available.width.into(), available.height.into()),
            );
            tf::Size {
                width: w,
                height: h,
            }
        },
    );

    // Walk the tree to collect absolute layouts.
    let mut layouts: HashMap<NodeId, BoxLayout> = HashMap::new();
    collect(&tree, root, &node_map, 0.0, 0.0, &mut layouts);

    LayoutResult { layouts }
}

fn build(
    tree: &mut TaffyTree<NodeData>,
    map: &mut HashMap<NodeId, tf::NodeId>,
    node: &BoxNode,
    ctx: &ConversionContext,
) -> tf::NodeId {
    let style = to_taffy_style(&node.css, ctx);
    let data = NodeData {
        box_id: node.id,
        intrinsic: node.intrinsic.clone(),
    };
    let tf_id = if node.intrinsic.is_some() {
        // Leaf with intrinsic measurement.
        tree.new_leaf_with_context(style, data)
            .expect("taffy new_leaf")
    } else if node.children.is_empty() {
        tree.new_leaf_with_context(style, data)
            .expect("taffy new_leaf")
    } else {
        let mut child_ids = Vec::with_capacity(node.children.len());
        for c in &node.children {
            child_ids.push(build(tree, map, c, ctx));
        }
        let id = tree
            .new_with_children(style, &child_ids)
            .expect("taffy new_with_children");
        // We still want context on internal nodes (for box_id mapping).
        tree.set_node_context(id, Some(data)).ok();
        id
    };
    map.insert(node.id, tf_id);
    tf_id
}

fn collect(
    tree: &TaffyTree<NodeData>,
    node: &BoxNode,
    map: &HashMap<NodeId, tf::NodeId>,
    parent_x: f32,
    parent_y: f32,
    out: &mut HashMap<NodeId, BoxLayout>,
) {
    let Some(&tf_id) = map.get(&node.id) else {
        return;
    };
    let Ok(layout) = tree.layout(tf_id) else {
        return;
    };

    let abs_x = parent_x + layout.location.x;
    let abs_y = parent_y + layout.location.y;
    let bx = BoxLayout {
        x: abs_x,
        y: abs_y,
        width: layout.size.width,
        height: layout.size.height,
        border: Insets {
            top: layout.border.top,
            right: layout.border.right,
            bottom: layout.border.bottom,
            left: layout.border.left,
        },
        padding: Insets {
            top: layout.padding.top,
            right: layout.padding.right,
            bottom: layout.padding.bottom,
            left: layout.padding.left,
        },
    };
    out.insert(node.id, bx);

    for c in &node.children {
        collect(tree, c, map, abs_x, abs_y, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::css::style::*;
    use crate::css::units::LengthPercentage;

    fn ctx() -> ConversionContext {
        ConversionContext::default()
    }

    #[test]
    fn single_block_takes_viewport() {
        let mut root = BoxNode::container(
            CssStyle {
                width: Some(Size::Length(LengthPercentage::String("100%".into()))),
                height: Some(Size::Length(LengthPercentage::String("100%".into()))),
                ..Default::default()
            },
            vec![],
        );
        root.assign_ids(1);
        let res = run_layout(&root, (1920.0, 1080.0), &ctx());
        let l = res.get(1).expect("root laid out");
        assert_eq!(l.width, 1920.0);
        assert_eq!(l.height, 1080.0);
        assert_eq!(l.x, 0.0);
        assert_eq!(l.y, 0.0);
    }

    #[test]
    fn flex_column_with_gap_stacks_children() {
        let mut root = BoxNode::container(
            CssStyle {
                display: Some(Display::Flex),
                flex_direction: Some(FlexDirection::Column),
                width: Some(Size::Length(LengthPercentage::Px(200.0))),
                height: Some(Size::Length(LengthPercentage::Px(400.0))),
                gap: Some(Gap::Uniform(LengthPercentage::Px(10.0))),
                ..Default::default()
            },
            vec![
                BoxNode::container(
                    CssStyle {
                        width: Some(Size::Length(LengthPercentage::Px(100.0))),
                        height: Some(Size::Length(LengthPercentage::Px(50.0))),
                        ..Default::default()
                    },
                    vec![],
                ),
                BoxNode::container(
                    CssStyle {
                        width: Some(Size::Length(LengthPercentage::Px(100.0))),
                        height: Some(Size::Length(LengthPercentage::Px(50.0))),
                        ..Default::default()
                    },
                    vec![],
                ),
            ],
        );
        root.assign_ids(1);
        let res = run_layout(&root, (200.0, 400.0), &ctx());
        let c1 = res.get(2).expect("c1");
        let c2 = res.get(3).expect("c2");
        assert_eq!(c1.x, 0.0);
        assert_eq!(c1.y, 0.0);
        assert_eq!(c2.x, 0.0);
        // 50px first child + 10px gap = 60
        assert_eq!(c2.y, 60.0);
    }

    #[test]
    fn padding_extends_content_box_inwards() {
        let mut root = BoxNode::container(
            CssStyle {
                width: Some(Size::Length(LengthPercentage::Px(200.0))),
                height: Some(Size::Length(LengthPercentage::Px(200.0))),
                padding: Some(Edges::Uniform(LengthPercentage::Px(20.0))),
                ..Default::default()
            },
            vec![],
        );
        root.assign_ids(1);
        let res = run_layout(&root, (1000.0, 1000.0), &ctx());
        let l = res.get(1).expect("layout");
        let (cx, cy, cw, ch) = l.content_box();
        assert_eq!(cx, 20.0);
        assert_eq!(cy, 20.0);
        assert_eq!(cw, 160.0);
        assert_eq!(ch, 160.0);
    }

    #[test]
    fn center_align_items_horizontally() {
        let mut root = BoxNode::container(
            CssStyle {
                display: Some(Display::Flex),
                flex_direction: Some(FlexDirection::Column),
                align_items: Some(AlignItems::Center),
                width: Some(Size::Length(LengthPercentage::Px(200.0))),
                height: Some(Size::Length(LengthPercentage::Px(200.0))),
                ..Default::default()
            },
            vec![BoxNode::container(
                CssStyle {
                    width: Some(Size::Length(LengthPercentage::Px(50.0))),
                    height: Some(Size::Length(LengthPercentage::Px(20.0))),
                    ..Default::default()
                },
                vec![],
            )],
        );
        root.assign_ids(1);
        let res = run_layout(&root, (200.0, 200.0), &ctx());
        let child = res.get(2).expect("child");
        // (200 - 50) / 2 = 75
        assert_eq!(child.x, 75.0);
    }

    #[test]
    fn position_absolute_inset() {
        let mut root = BoxNode::container(
            CssStyle {
                width: Some(Size::Length(LengthPercentage::Px(400.0))),
                height: Some(Size::Length(LengthPercentage::Px(400.0))),
                ..Default::default()
            },
            vec![BoxNode::container(
                CssStyle {
                    position: Some(Position::Absolute),
                    top: Some(LengthPercentage::Px(30.0)),
                    left: Some(LengthPercentage::Px(40.0)),
                    width: Some(Size::Length(LengthPercentage::Px(100.0))),
                    height: Some(Size::Length(LengthPercentage::Px(80.0))),
                    ..Default::default()
                },
                vec![],
            )],
        );
        root.assign_ids(1);
        let res = run_layout(&root, (400.0, 400.0), &ctx());
        let child = res.get(2).expect("child");
        assert_eq!(child.x, 40.0);
        assert_eq!(child.y, 30.0);
        assert_eq!(child.width, 100.0);
        assert_eq!(child.height, 80.0);
    }
}
