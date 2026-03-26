use crate::traits::LayoutItem;

use super::{Constraints, LayoutNode};

/// Compute layout for a stack container.
/// All children are positioned absolutely — each at its declared position (or 0,0).
pub fn layout_stack<L: LayoutItem>(
    children: &[L],
    style: &crate::schema::LayerStyle,
    constraints: &Constraints,
) -> LayoutNode {
    let (pt, pr, pb, pl) = style
        .padding
        .as_ref()
        .map(|p| p.resolve())
        .unwrap_or_default();

    let container_w = if constraints.has_bounded_width() {
        constraints.max_width
    } else {
        // Size from children bounding box
        let mut max_x = 0.0f32;
        for child in children {
            let (ax, _ay) = child_position(child);
            let (cw, _) = measure_child(child, constraints);
            max_x = max_x.max(ax + cw);
        }
        max_x + pl + pr
    };
    let container_h = if constraints.has_bounded_height() {
        constraints.max_height
    } else {
        let mut max_y = 0.0f32;
        for child in children {
            let (_, ay) = child_position(child);
            let (_, ch) = measure_child(child, constraints);
            max_y = max_y.max(ay + ch);
        }
        max_y + pt + pb
    };

    let child_nodes: Vec<LayoutNode> = children
        .iter()
        .map(|child| {
            let (ax, ay) = child_position(child);
            let (cw, ch) = measure_child(child, constraints);
            LayoutNode::new(pl + ax, pt + ay, cw, ch)
        })
        .collect();

    LayoutNode::new(0.0, 0.0, container_w, container_h).with_children(child_nodes)
}

fn child_position<L: LayoutItem>(child: &L) -> (f32, f32) {
    child
        .absolute_position()
        .unwrap_or_else(|| child.default_position())
}

fn measure_child<L: LayoutItem>(child: &L, constraints: &Constraints) -> (f32, f32) {
    let widget = child.as_widget();
    let styled = child.as_styled();
    let (w, h) = widget.measure(constraints);
    if child.is_container() {
        let (mt, mr, mb, ml) = styled.margin();
        (w + ml + mr, h + mt + mb)
    } else {
        let (pt, pr, pb, pl) = styled.padding();
        let (mt, mr, mb, ml) = styled.margin();
        (w + pl + pr + ml + mr, h + pt + pb + mt + mb)
    }
}
