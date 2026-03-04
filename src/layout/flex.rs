use crate::components::ChildComponent;
use crate::traits::{Align, Container, Direction, Justify, Styled};

use super::{Constraints, LayoutNode};

/// Helper to get per-child flex props from the child's component style.
fn child_flex_basis(child: &ChildComponent) -> Option<f32> {
    child.component.as_styled().style_config().flex_basis
}
fn child_flex_grow(child: &ChildComponent) -> Option<f32> {
    child.component.as_styled().style_config().flex_grow
}
fn child_flex_shrink(child: &ChildComponent) -> Option<f32> {
    child.component.as_styled().style_config().flex_shrink
}
fn child_align_self(child: &ChildComponent) -> Option<Align> {
    child.component.as_styled().style_config().align_self.clone()
}

/// Measure a single child component: widget measure + padding + margin.
fn measure_child(child: &ChildComponent, constraints: &Constraints) -> (f32, f32) {
    let widget = child.component.as_widget();
    let styled = child.component.as_styled();
    let (w, h) = widget.measure(constraints);
    let (pt, pr, pb, pl) = styled.padding();
    let (mt, mr, mb, ml) = styled.margin();
    (w + pl + pr + ml + mr, h + pt + pb + mt + mb)
}

/// Recursively compute child layouts for container children.
/// Takes a parent LayoutNode with flat child nodes (position + size only)
/// and enriches each child with its own recursive layout tree.
pub(crate) fn enrich_child_layouts(parent: LayoutNode, children: &[ChildComponent]) -> LayoutNode {
    let enriched: Vec<LayoutNode> = parent
        .children
        .into_iter()
        .enumerate()
        .map(|(i, node)| {
            if i < children.len() {
                let child_constraints = Constraints::tight(node.width, node.height);
                let mut enriched = children[i].component.as_widget().layout(&child_constraints);
                enriched.x = node.x;
                enriched.y = node.y;
                // Preserve the parent-computed size (may differ from natural size due to stretch/grow)
                enriched.width = node.width;
                enriched.height = node.height;
                enriched
            } else {
                node
            }
        })
        .collect();
    LayoutNode::new(parent.x, parent.y, parent.width, parent.height).with_children(enriched)
}

/// Compute layout for a flex container's children.
/// Returns a `LayoutNode` for the container with children positioned.
pub fn layout_flex(
    container: &(impl Container + Styled + ?Sized),
    constraints: &Constraints,
) -> LayoutNode {
    let children = container.children();
    if children.is_empty() {
        let (w, h) = constraints.constrain(0.0, 0.0);
        return LayoutNode::new(0.0, 0.0, w, h);
    }

    let styled = container.style_config();
    let (pt, pr, pb, pl) = styled
        .padding
        .as_ref()
        .map(|p| p.resolve())
        .unwrap_or_default();

    let direction = styled.flex_direction_or(Direction::Column);
    let justify = styled.justify_content_or(Justify::Start);
    let align = styled.align_items_or(Align::Start);
    let gap = styled.gap_or(0.0);
    let wrap = styled.flex_wrap_or(false);

    let is_row = matches!(direction, Direction::Row | Direction::RowReverse);
    let is_reverse = matches!(
        direction,
        Direction::RowReverse | Direction::ColumnReverse
    );

    // Available content area
    let content_max_w = if constraints.has_bounded_width() {
        constraints.max_width - pl - pr
    } else {
        f32::INFINITY
    };
    let content_max_h = if constraints.has_bounded_height() {
        constraints.max_height - pt - pb
    } else {
        f32::INFINITY
    };

    // Separate flow and absolute children
    let flow_indices: Vec<usize> = children
        .iter()
        .enumerate()
        .filter(|(_, c)| c.is_flow())
        .map(|(i, _)| i)
        .collect();

    // Measure flow children
    let child_constraints = Constraints::loose(content_max_w, content_max_h);
    let child_sizes: Vec<(f32, f32)> = children
        .iter()
        .map(|c| measure_child(c, &child_constraints))
        .collect();

    if !wrap {
        // Single-line flex
        let flow_sizes: Vec<(f32, f32)> = flow_indices.iter().map(|&i| child_sizes[i]).collect();
        let line_results = layout_single_line(
            children,
            &flow_indices,
            &flow_sizes,
            is_row,
            &justify,
            &align,
            gap,
            content_max_w,
            content_max_h,
        );

        // Compute content size
        let (content_w, content_h) = compute_content_size(&flow_sizes, is_row, gap);
        let container_w = if constraints.has_bounded_width() {
            constraints.max_width
        } else {
            content_w + pl + pr
        };
        let container_h = if constraints.has_bounded_height() {
            constraints.max_height
        } else {
            content_h + pt + pb
        };

        let mut child_nodes = build_child_nodes(
            children,
            &child_sizes,
            &flow_indices,
            &line_results,
            is_reverse,
            is_row,
            content_max_w,
            content_max_h,
            pl,
            pt,
            &child_constraints,
        );

        // Handle absolute children
        layout_absolute_children(children, &child_sizes, &mut child_nodes, pl, pt);

        let flat = LayoutNode::new(0.0, 0.0, container_w, container_h).with_children(child_nodes);
        return enrich_child_layouts(flat, children);
    }

    // Wrap mode: partition flow children into lines
    let main_limit = if is_row { content_max_w } else { content_max_h };
    let mut lines: Vec<Vec<usize>> = vec![vec![]];
    let mut current_main = 0.0f32;

    for &fi in &flow_indices {
        let (cw, ch) = child_sizes[fi];
        let child_main = if is_row { cw } else { ch };
        let needed = if lines.last().unwrap().is_empty() {
            child_main
        } else {
            current_main + gap + child_main
        };

        if needed > main_limit && !lines.last().unwrap().is_empty() {
            lines.push(vec![fi]);
            current_main = child_main;
        } else {
            lines.last_mut().unwrap().push(fi);
            current_main = needed;
        }
    }

    // Layout each line
    let mut child_nodes: Vec<Option<LayoutNode>> = (0..children.len()).map(|_| None).collect();
    let mut cross_offset = 0.0f32;

    for line in &lines {
        let line_sizes: Vec<(f32, f32)> = line.iter().map(|&i| child_sizes[i]).collect();
        let line_cross = line_sizes
            .iter()
            .map(|&(w, h)| if is_row { h } else { w })
            .fold(0.0f32, f32::max);

        let line_results = layout_single_line(
            children,
            line,
            &line_sizes,
            is_row,
            &justify,
            &align,
            gap,
            content_max_w,
            content_max_h,
        );

        for (j, &idx) in line.iter().enumerate() {
            let mut node = line_results[j].clone();
            let child_as = child_align_self(&children[idx]);
            let child_align_ref = child_as.as_ref().unwrap_or(&align);

            if is_row {
                let (cross_pos, stretch_h) = align_item(line_sizes[j].1, line_cross, child_align_ref);
                node.y = cross_offset + cross_pos;
                if let Some(h) = stretch_h {
                    node.height = h;
                }
            } else {
                let (cross_pos, stretch_w) = align_item(line_sizes[j].0, line_cross, child_align_ref);
                node.x = cross_offset + cross_pos;
                if let Some(w) = stretch_w {
                    node.width = w;
                }
            }

            // Add padding offset
            node.x += pl;
            node.y += pt;
            child_nodes[idx] = Some(node);
        }

        cross_offset += line_cross + gap;
    }

    // Handle absolute children
    for (i, child) in children.iter().enumerate() {
        if let Some((ax, ay)) = child.absolute_position() {
            let (cw, ch) = child_sizes[i];
            child_nodes[i] = Some(LayoutNode::new(pl + ax, pt + ay, cw, ch));
        }
    }

    // Fill remaining None entries with zero-sized nodes
    let final_nodes: Vec<LayoutNode> = child_nodes
        .into_iter()
        .map(|n| n.unwrap_or_default())
        .collect();

    let container_w = if constraints.has_bounded_width() {
        constraints.max_width
    } else {
        // Sum up the widest line + padding
        let max_w = lines
            .iter()
            .map(|line| {
                let sizes: Vec<(f32, f32)> = line.iter().map(|&i| child_sizes[i]).collect();
                compute_content_size(&sizes, is_row, gap).0
            })
            .fold(0.0f32, f32::max);
        max_w + pl + pr
    };
    let container_h = if constraints.has_bounded_height() {
        constraints.max_height
    } else {
        cross_offset - gap + pt + pb
    };

    let flat = LayoutNode::new(0.0, 0.0, container_w, container_h).with_children(final_nodes);
    enrich_child_layouts(flat, children)
}

/// Compute the natural content size of a list of children in a given direction.
fn compute_content_size(sizes: &[(f32, f32)], is_row: bool, gap: f32) -> (f32, f32) {
    if sizes.is_empty() {
        return (0.0, 0.0);
    }
    let gap_total = gap * (sizes.len() as f32 - 1.0).max(0.0);
    if is_row {
        let total_w: f32 = sizes.iter().map(|(w, _)| *w).sum::<f32>() + gap_total;
        let max_h = sizes.iter().map(|(_, h)| *h).fold(0.0f32, f32::max);
        (total_w, max_h)
    } else {
        let max_w = sizes.iter().map(|(w, _)| *w).fold(0.0f32, f32::max);
        let total_h: f32 = sizes.iter().map(|(_, h)| *h).sum::<f32>() + gap_total;
        (max_w, total_h)
    }
}

/// Layout children in a single flex line.
/// Returns one `LayoutNode` per entry in `indices`, positioned relative to content origin (0,0).
fn layout_single_line(
    all_children: &[ChildComponent],
    indices: &[usize],
    sizes: &[(f32, f32)],
    is_row: bool,
    justify: &Justify,
    align: &Align,
    gap: f32,
    container_main_size: f32,
    container_cross_size: f32,
) -> Vec<LayoutNode> {
    let n = sizes.len();
    if n == 0 {
        return vec![];
    }

    // main axis is the direction axis
    let container_main = if is_row {
        container_main_size
    } else {
        container_cross_size
    };
    let container_cross = if is_row {
        container_cross_size
    } else {
        container_main_size
    };

    // Compute base main sizes (flex_basis or natural)
    let mut main_sizes: Vec<f32> = Vec::with_capacity(n);
    for (j, &idx) in indices.iter().enumerate() {
        let natural = if is_row { sizes[j].0 } else { sizes[j].1 };
        let basis = child_flex_basis(&all_children[idx]).unwrap_or(natural);
        main_sizes.push(basis);
    }
    let cross_sizes: Vec<f32> = sizes
        .iter()
        .map(|&(w, h)| if is_row { h } else { w })
        .collect();

    let total_main: f32 = main_sizes.iter().sum::<f32>() + gap * (n as f32 - 1.0).max(0.0);
    let remaining = container_main - total_main;

    // flex_grow / flex_shrink
    if remaining > 0.0 {
        let total_grow: f32 = indices
            .iter()
            .map(|&idx| child_flex_grow(&all_children[idx]).unwrap_or(0.0))
            .sum();
        if total_grow > 0.0 {
            for (j, &idx) in indices.iter().enumerate() {
                let grow = child_flex_grow(&all_children[idx]).unwrap_or(0.0);
                if grow > 0.0 {
                    main_sizes[j] += remaining * (grow / total_grow);
                }
            }
        }
    } else if remaining < 0.0 {
        let overflow = -remaining;
        let weighted_total: f32 = indices
            .iter()
            .enumerate()
            .map(|(j, &idx)| {
                let shrink = child_flex_shrink(&all_children[idx]).unwrap_or(1.0);
                main_sizes[j] * shrink
            })
            .sum();
        if weighted_total > 0.0 {
            for (j, &idx) in indices.iter().enumerate() {
                let shrink = child_flex_shrink(&all_children[idx]).unwrap_or(1.0);
                let weight = main_sizes[j] * shrink;
                main_sizes[j] = (main_sizes[j] - overflow * weight / weighted_total).max(0.0);
            }
        }
    }

    // Justify
    let actual_total: f32 = main_sizes.iter().sum::<f32>() + gap * (n as f32 - 1.0).max(0.0);
    let new_remaining = (container_main - actual_total).max(0.0);

    let (mut main_pos, effective_gap) = match justify {
        Justify::Start => (0.0, gap),
        Justify::Center => (new_remaining / 2.0, gap),
        Justify::End => (new_remaining, gap),
        Justify::SpaceBetween => {
            if n <= 1 {
                (0.0, gap)
            } else {
                let total_no_gap: f32 = main_sizes.iter().sum();
                let space = (container_main - total_no_gap).max(0.0) / (n as f32 - 1.0);
                (0.0, space)
            }
        }
        Justify::SpaceAround => {
            if n == 0 {
                (0.0, gap)
            } else {
                let total_no_gap: f32 = main_sizes.iter().sum();
                let space = (container_main - total_no_gap).max(0.0) / n as f32;
                (space / 2.0, space)
            }
        }
        Justify::SpaceEvenly => {
            let total_no_gap: f32 = main_sizes.iter().sum();
            let space = (container_main - total_no_gap).max(0.0) / (n as f32 + 1.0);
            (space, space)
        }
    };

    let mut result = Vec::with_capacity(n);
    for (j, &idx) in indices.iter().enumerate() {
        let child_as = child_align_self(&all_children[idx]);
        let child_align_val = child_as.as_ref().unwrap_or(align);
        let (cross_pos, stretch_size) = align_item(cross_sizes[j], container_cross, child_align_val);

        let (x, y) = if is_row {
            (main_pos, cross_pos)
        } else {
            (cross_pos, main_pos)
        };

        let w = if is_row {
            main_sizes[j]
        } else {
            stretch_size.unwrap_or(cross_sizes[j])
        };
        let h = if is_row {
            stretch_size.unwrap_or(cross_sizes[j])
        } else {
            main_sizes[j]
        };

        result.push(LayoutNode::new(x, y, w, h));
        main_pos += main_sizes[j] + effective_gap;
    }

    result
}

/// Align a single item on the cross axis.
/// Returns (position, optional stretch size).
fn align_item(item_size: f32, container_size: f32, align: &Align) -> (f32, Option<f32>) {
    match align {
        Align::Start => (0.0, None),
        Align::Center => ((container_size - item_size).max(0.0) / 2.0, None),
        Align::End => ((container_size - item_size).max(0.0), None),
        Align::Stretch => (0.0, Some(container_size)),
    }
}

/// Build child layout nodes from single-line results, applying reverse and padding offsets.
fn build_child_nodes(
    children: &[ChildComponent],
    _child_sizes: &[(f32, f32)],
    flow_indices: &[usize],
    line_results: &[LayoutNode],
    is_reverse: bool,
    is_row: bool,
    content_w: f32,
    content_h: f32,
    pad_l: f32,
    pad_t: f32,
    _constraints: &Constraints,
) -> Vec<LayoutNode> {
    let mut child_nodes: Vec<LayoutNode> = (0..children.len())
        .map(|_| LayoutNode::default())
        .collect();

    let ordered: Vec<(usize, usize)> = if is_reverse {
        flow_indices
            .iter()
            .rev()
            .enumerate()
            .map(|(j, &i)| (j, i))
            .collect()
    } else {
        flow_indices.iter().enumerate().map(|(j, &i)| (j, i)).collect()
    };

    for (j, idx) in ordered {
        let mut node = line_results[j].clone();
        if is_reverse {
            if is_row {
                node.x = content_w - node.x - node.width;
            } else {
                node.y = content_h - node.y - node.height;
            }
        }
        node.x += pad_l;
        node.y += pad_t;
        child_nodes[idx] = node;
    }

    child_nodes
}

/// Position absolute children at their declared coordinates.
fn layout_absolute_children(
    children: &[ChildComponent],
    child_sizes: &[(f32, f32)],
    child_nodes: &mut [LayoutNode],
    pad_l: f32,
    pad_t: f32,
) {
    for (i, child) in children.iter().enumerate() {
        if let Some((ax, ay)) = child.absolute_position() {
            let (cw, ch) = child_sizes[i];
            child_nodes[i] = LayoutNode::new(pad_l + ax, pad_t + ay, cw, ch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::flex::{Flex, FlexSize};
    use crate::components::shape::Shape;
    use crate::components::{ChildComponent, Component, PositionMode};
    use crate::schema::{ShapeType, Size, SizeDimension, LayerStyle};

    /// Helper to create a shape child with given size.
    fn shape_child(w: f32, h: f32) -> ChildComponent {
        ChildComponent {
            component: Component::Shape(Shape {
                shape: ShapeType::Rect,
                size: Size { width: w, height: h },
                text: None,
                style: LayerStyle::default(),
                animation: Default::default(),
                timing: Default::default(),
            }),
            position: None,
            x: None,
            y: None,
        }
    }

    fn shape_child_grow(w: f32, h: f32, grow: f32) -> ChildComponent {
        let mut c = shape_child(w, h);
        if let Component::Shape(ref mut s) = c.component {
            s.style.flex_grow = Some(grow);
        }
        c
    }

    fn shape_child_absolute(w: f32, h: f32, x: f32, y: f32) -> ChildComponent {
        let mut c = shape_child(w, h);
        c.position = Some(PositionMode::Absolute { x, y });
        c
    }

    fn make_flex(direction: Direction, children: Vec<ChildComponent>) -> Flex {
        let mut style = LayerStyle::default();
        style.flex_direction = Some(direction);
        Flex {
            children,
            size: Some(FlexSize {
                width: SizeDimension::Fixed(400.0),
                height: SizeDimension::Fixed(300.0),
            }),
            animation: Default::default(),
            timing: Default::default(),
            style,
        }
    }

    #[test]
    fn test_column_3_children() {
        let flex = make_flex(
            Direction::Column,
            vec![shape_child(100.0, 50.0), shape_child(80.0, 60.0), shape_child(120.0, 40.0)],
        );
        let constraints = Constraints::tight(400.0, 300.0);
        let result = layout_flex(&flex, &constraints);

        assert_eq!(result.children.len(), 3);
        // Column: stacked vertically
        assert_eq!(result.children[0].y, 0.0);
        assert_eq!(result.children[1].y, 50.0);
        assert_eq!(result.children[2].y, 110.0); // 50 + 60
    }

    #[test]
    fn test_row_3_children() {
        let flex = make_flex(
            Direction::Row,
            vec![shape_child(100.0, 50.0), shape_child(80.0, 60.0), shape_child(120.0, 40.0)],
        );
        let constraints = Constraints::tight(400.0, 300.0);
        let result = layout_flex(&flex, &constraints);

        assert_eq!(result.children.len(), 3);
        // Row: side by side
        assert_eq!(result.children[0].x, 0.0);
        assert_eq!(result.children[1].x, 100.0);
        assert_eq!(result.children[2].x, 180.0); // 100 + 80
    }

    #[test]
    fn test_row_with_gap() {
        let mut flex = make_flex(
            Direction::Row,
            vec![shape_child(100.0, 50.0), shape_child(80.0, 60.0), shape_child(120.0, 40.0)],
        );
        flex.style.gap = Some(10.0);
        let constraints = Constraints::tight(400.0, 300.0);
        let result = layout_flex(&flex, &constraints);

        assert_eq!(result.children[0].x, 0.0);
        assert_eq!(result.children[1].x, 110.0); // 100 + 10 gap
        assert_eq!(result.children[2].x, 200.0); // 110 + 80 + 10 gap
    }

    #[test]
    fn test_justify_center() {
        let mut flex = make_flex(
            Direction::Row,
            vec![shape_child(100.0, 50.0)],
        );
        flex.style.justify_content = Some(Justify::Center);
        let constraints = Constraints::tight(400.0, 300.0);
        let result = layout_flex(&flex, &constraints);

        assert_eq!(result.children[0].x, 150.0); // (400 - 100) / 2
    }

    #[test]
    fn test_justify_space_between() {
        let mut flex = make_flex(
            Direction::Row,
            vec![shape_child(50.0, 50.0), shape_child(50.0, 50.0), shape_child(50.0, 50.0)],
        );
        flex.style.justify_content = Some(Justify::SpaceBetween);
        let constraints = Constraints::tight(400.0, 300.0);
        let result = layout_flex(&flex, &constraints);

        assert_eq!(result.children[0].x, 0.0);
        // Space = (400 - 150) / 2 = 125
        assert!((result.children[1].x - 175.0).abs() < 0.01);
        assert!((result.children[2].x - 350.0).abs() < 0.01);
    }

    #[test]
    fn test_justify_space_evenly() {
        let mut flex = make_flex(
            Direction::Row,
            vec![shape_child(50.0, 50.0), shape_child(50.0, 50.0)],
        );
        flex.style.justify_content = Some(Justify::SpaceEvenly);
        let constraints = Constraints::tight(300.0, 300.0);
        let result = layout_flex(&flex, &constraints);

        // Space = (300 - 100) / 3 = 66.67
        let space = 200.0 / 3.0;
        assert!((result.children[0].x - space).abs() < 0.01);
        assert!((result.children[1].x - (space * 2.0 + 50.0)).abs() < 0.01);
    }

    #[test]
    fn test_align_center() {
        let mut flex = make_flex(
            Direction::Row,
            vec![shape_child(100.0, 50.0)],
        );
        flex.style.align_items = Some(Align::Center);
        let constraints = Constraints::tight(400.0, 300.0);
        let result = layout_flex(&flex, &constraints);

        assert_eq!(result.children[0].y, 125.0); // (300 - 50) / 2
    }

    #[test]
    fn test_align_stretch() {
        let mut flex = make_flex(
            Direction::Row,
            vec![shape_child(100.0, 50.0)],
        );
        flex.style.align_items = Some(Align::Stretch);
        let constraints = Constraints::tight(400.0, 300.0);
        let result = layout_flex(&flex, &constraints);

        assert_eq!(result.children[0].y, 0.0);
        assert_eq!(result.children[0].height, 300.0);
    }

    #[test]
    fn test_flex_grow() {
        let flex = make_flex(
            Direction::Row,
            vec![shape_child_grow(50.0, 50.0, 1.0), shape_child_grow(50.0, 50.0, 1.0)],
        );
        let constraints = Constraints::tight(400.0, 300.0);
        let result = layout_flex(&flex, &constraints);

        // Remaining = 400 - 100 = 300, split evenly
        assert!((result.children[0].width - 200.0).abs() < 0.01);
        assert!((result.children[1].width - 200.0).abs() < 0.01);
    }

    #[test]
    fn test_absolute_child_in_flex() {
        let flex = make_flex(
            Direction::Column,
            vec![
                shape_child(100.0, 50.0),
                shape_child_absolute(80.0, 80.0, 200.0, 150.0),
                shape_child(100.0, 50.0),
            ],
        );
        let constraints = Constraints::tight(400.0, 300.0);
        let result = layout_flex(&flex, &constraints);

        // Flow children: 0 and 2
        assert_eq!(result.children[0].y, 0.0);
        assert_eq!(result.children[2].y, 50.0); // right after first, absolute skipped

        // Absolute child at declared position
        assert_eq!(result.children[1].x, 200.0);
        assert_eq!(result.children[1].y, 150.0);
    }

    #[test]
    fn test_flex_wrap() {
        let mut flex = make_flex(
            Direction::Row,
            vec![
                shape_child(200.0, 50.0),
                shape_child(200.0, 50.0),
                shape_child(200.0, 50.0),
            ],
        );
        flex.style.flex_wrap = Some(true);
        let constraints = Constraints::tight(400.0, 300.0);
        let result = layout_flex(&flex, &constraints);

        // First line: children 0, 1 (200 + 200 = 400, fits)
        // Second line: child 2
        assert_eq!(result.children[0].y, 0.0);
        assert_eq!(result.children[1].y, 0.0);
        assert_eq!(result.children[2].y, 50.0); // next line
    }

    #[test]
    fn test_nested_flex() {
        // Outer flex (column) containing inner flex (row) containing shapes
        let mut inner_style = LayerStyle::default();
        inner_style.flex_direction = Some(Direction::Row);
        let inner = Flex {
            children: vec![shape_child(50.0, 30.0), shape_child(50.0, 30.0)],
            size: None,
            animation: Default::default(),
            timing: Default::default(),
            style: inner_style,
        };

        let outer_children = vec![
            ChildComponent {
                component: Component::Flex(inner),
                position: None,
                x: None,
                y: None,
            },
            shape_child(100.0, 40.0),
        ];

        let outer = make_flex(Direction::Column, outer_children);
        let constraints = Constraints::tight(400.0, 300.0);
        let result = layout_flex(&outer, &constraints);

        assert_eq!(result.children.len(), 2);
        // Inner flex measures as (0, 0) for now since Widget::measure is a stub
        // but the structure is correct
    }
}
