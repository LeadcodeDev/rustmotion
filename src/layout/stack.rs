#![allow(dead_code)]
use crate::components::ChildComponent;
use crate::traits::{Container, Styled};

use super::{Constraints, LayoutNode};

/// Compute layout for a stack container.
/// All children are positioned absolutely — each at its declared position (or 0,0).
pub fn layout_stack(
    container: &(impl Container + Styled + ?Sized),
    constraints: &Constraints,
) -> LayoutNode {
    let children = container.children();
    let styled = container.style_config();
    let (pt, pr, pb, pl) = styled
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

fn child_position(child: &ChildComponent) -> (f32, f32) {
    child
        .absolute_position()
        .or_else(|| Some((child.x.unwrap_or(0.0), child.y.unwrap_or(0.0))))
        .unwrap()
}

fn measure_child(child: &ChildComponent, constraints: &Constraints) -> (f32, f32) {
    let widget = child.component.as_widget();
    let styled = child.component.as_styled();
    let (w, h) = widget.measure(constraints);
    let (pt, pr, pb, pl) = styled.padding();
    let (mt, mr, mb, ml) = styled.margin();
    (w + pl + pr + ml + mr, h + pt + pb + mt + mb)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::shape::Shape;
    use crate::components::stack::Stack;
    use crate::components::{ChildComponent, Component, PositionMode};
    use crate::schema::{LayerStyle, ShapeType, Size};

    fn shape_child_at(w: f32, h: f32, x: f32, y: f32) -> ChildComponent {
        ChildComponent {
            component: Component::Shape(Shape {
                shape: ShapeType::Rect,
                size: Size { width: w, height: h },
                text: None,
                style: LayerStyle::default(),
                animation: Default::default(),
                timing: Default::default(),
            }),
            position: Some(PositionMode::Absolute { x, y }),
            x: None,
            y: None,
        }
    }

    #[test]
    fn test_stack_2_children() {
        let stack = Stack {
            children: vec![
                shape_child_at(100.0, 100.0, 0.0, 0.0),
                shape_child_at(50.0, 50.0, 200.0, 150.0),
            ],
            style: LayerStyle::default(),
        };
        let constraints = Constraints::tight(400.0, 300.0);
        let result = layout_stack(&stack, &constraints);

        assert_eq!(result.children.len(), 2);
        assert_eq!(result.children[0].x, 0.0);
        assert_eq!(result.children[0].y, 0.0);
        assert_eq!(result.children[1].x, 200.0);
        assert_eq!(result.children[1].y, 150.0);
    }
}
