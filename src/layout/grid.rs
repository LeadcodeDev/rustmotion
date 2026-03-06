use crate::components::ChildComponent;
use crate::traits::{Align, Container, GridConfig, GridTrack, Styled};

use super::{Constraints, LayoutNode};

/// Helper to get per-child grid placement from the child's component style.
fn child_grid_column(child: &ChildComponent) -> Option<&crate::schema::GridPlacement> {
    child.component.as_styled().style_config().grid_column.as_ref()
}
fn child_grid_row(child: &ChildComponent) -> Option<&crate::schema::GridPlacement> {
    child.component.as_styled().style_config().grid_row.as_ref()
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

/// Compute layout for a grid container's children.
pub fn layout_grid(
    container: &(impl Container + Styled + ?Sized),
    constraints: &Constraints,
) -> LayoutNode {
    let styled = container.style_config();
    let config = GridConfig {
        grid_template_columns: styled.grid_template_columns.clone(),
        grid_template_rows: styled.grid_template_rows.clone(),
        gap: styled.gap_or(0.0),
    };
    layout_grid_with_config(container, &config, constraints)
}

/// Compute layout for a container using the given grid config.
/// Useful for components like Card that manage grid config separately.
pub fn layout_grid_with_config(
    container: &(impl Container + Styled + ?Sized),
    config: &GridConfig,
    constraints: &Constraints,
) -> LayoutNode {
    let children = container.children();
    let n = children.len();
    if n == 0 {
        let (w, h) = constraints.constrain(0.0, 0.0);
        return LayoutNode::new(0.0, 0.0, w, h);
    }

    let styled = container.style_config();
    let (pt, pr, pb, pl) = styled
        .padding
        .as_ref()
        .map(|p| p.resolve())
        .unwrap_or_default();

    let container_w = if constraints.has_bounded_width() {
        constraints.max_width - pl - pr
    } else {
        600.0 // default fallback
    };
    let container_h = if constraints.has_bounded_height() {
        constraints.max_height - pt - pb
    } else {
        400.0 // default fallback
    };

    let default_col = [GridTrack::Fr(1.0)];
    let default_row = [GridTrack::Fr(1.0)];
    let col_tracks = config
        .grid_template_columns
        .as_deref()
        .unwrap_or(&default_col);
    let row_tracks = config
        .grid_template_rows
        .as_deref()
        .unwrap_or(&default_row);

    let num_cols = col_tracks.len().max(1);
    let num_rows = row_tracks.len().max(1);

    // Measure children
    let child_constraints = Constraints::loose(container_w, container_h);
    let child_sizes: Vec<(f32, f32)> = children
        .iter()
        .map(|c| measure_child(c, &child_constraints))
        .collect();

    // Place children in grid cells
    let mut placements: Vec<(usize, usize, usize, usize)> = Vec::with_capacity(n);
    let mut grid_occupied: Vec<Vec<bool>> = vec![vec![false; num_cols]; num_rows * 2];
    let mut auto_cursor = (0usize, 0usize);

    for child in children {
        let gc = child_grid_column(child);
        let gr = child_grid_row(child);
        let col_start = gc
            .and_then(|g| g.start)
            .map(|s| (s - 1).max(0) as usize);
        let row_start = gr
            .and_then(|g| g.start)
            .map(|s| (s - 1).max(0) as usize);
        let col_span = gc
            .and_then(|g| g.span)
            .unwrap_or(1)
            .max(1) as usize;
        let row_span = gr
            .and_then(|g| g.span)
            .unwrap_or(1)
            .max(1) as usize;

        if let (Some(c), Some(r)) = (col_start, row_start) {
            let c = c.min(num_cols - 1);
            let cs = col_span.min(num_cols - c);
            placements.push((c, r, cs, row_span));
            mark_occupied(&mut grid_occupied, r, c, row_span, cs, num_cols);
        } else if let Some(c) = col_start {
            let c = c.min(num_cols - 1);
            let r = auto_cursor.0;
            let cs = col_span.min(num_cols - c);
            placements.push((c, r, cs, row_span));
            mark_occupied(&mut grid_occupied, r, c, row_span, cs, num_cols);
        } else if let Some(r) = row_start {
            let mut c = 0;
            while c < num_cols && r < grid_occupied.len() && grid_occupied[r][c] {
                c += 1;
            }
            let c = c.min(num_cols - 1);
            let cs = col_span.min(num_cols - c);
            placements.push((c, r, cs, row_span));
            mark_occupied(&mut grid_occupied, r, c, row_span, cs, num_cols);
        } else {
            // Auto placement
            let (mut ar, mut ac) = auto_cursor;
            loop {
                if ar >= grid_occupied.len() {
                    grid_occupied.push(vec![false; num_cols]);
                }
                if !grid_occupied[ar][ac] {
                    let mut fits = true;
                    for dc in 0..col_span {
                        if ac + dc >= num_cols {
                            fits = false;
                            break;
                        }
                    }
                    if fits {
                        break;
                    }
                }
                ac += 1;
                if ac >= num_cols {
                    ac = 0;
                    ar += 1;
                }
            }
            let cs = col_span.min(num_cols - ac);
            placements.push((ac, ar, cs, row_span));
            mark_occupied(&mut grid_occupied, ar, ac, row_span, cs, num_cols);
            ac += col_span;
            if ac >= num_cols {
                ac = 0;
                ar += 1;
            }
            auto_cursor = (ar, ac);
        }
    }

    // Determine actual number of rows
    let actual_num_rows = placements
        .iter()
        .map(|&(_, r, _, rs)| r + rs)
        .max()
        .unwrap_or(num_rows)
        .max(num_rows);

    // Resolve track sizes
    let col_sizes = resolve_tracks(
        col_tracks,
        container_w,
        config.gap,
        num_cols,
        &child_sizes,
        &placements,
        true,
    );

    let mut extended_row_tracks: Vec<GridTrack> = row_tracks.to_vec();
    while extended_row_tracks.len() < actual_num_rows {
        extended_row_tracks.push(GridTrack::Auto);
    }
    let row_sizes = resolve_tracks(
        &extended_row_tracks,
        container_h,
        config.gap,
        actual_num_rows,
        &child_sizes,
        &placements,
        false,
    );

    // Compute cell positions
    let mut col_offsets = vec![0.0f32; num_cols + 1];
    for i in 0..num_cols {
        col_offsets[i + 1] = col_offsets[i] + col_sizes[i] + config.gap;
    }
    let mut row_offsets = vec![0.0f32; actual_num_rows + 1];
    for i in 0..actual_num_rows {
        row_offsets[i + 1] = row_offsets[i] + row_sizes[i] + config.gap;
    }

    let mut child_nodes = Vec::with_capacity(n);
    for (i, &(col, row, col_span, row_span)) in placements.iter().enumerate() {
        let x = col_offsets[col];
        let y = row_offsets[row];
        let end_col = (col + col_span).min(num_cols);
        let end_row = (row + row_span).min(actual_num_rows);
        let w = (col_offsets[end_col] - col_offsets[col] - config.gap).max(0.0);
        let h = (row_offsets[end_row] - row_offsets[row] - config.gap).max(0.0);

        // Align child within cell
        let (cw, ch) = child_sizes[i];
        let (cx, _) = align_item(cw, w, &Align::Start);
        let (cy, _) = align_item(ch, h, &Align::Start);

        child_nodes.push(LayoutNode::new(pl + x + cx, pt + y + cy, w, h));
    }

    let total_w = if constraints.has_bounded_width() {
        constraints.max_width
    } else {
        col_offsets[num_cols] - config.gap + pl + pr
    };
    let total_h = if constraints.has_bounded_height() {
        constraints.max_height
    } else {
        row_offsets[actual_num_rows] - config.gap + pt + pb
    };

    let flat = LayoutNode::new(0.0, 0.0, total_w, total_h).with_children(child_nodes);
    super::flex::enrich_child_layouts(flat, children)
}

fn mark_occupied(
    grid: &mut Vec<Vec<bool>>,
    row: usize,
    col: usize,
    row_span: usize,
    col_span: usize,
    num_cols: usize,
) {
    for dr in 0..row_span {
        for dc in 0..col_span {
            let rr = row + dr;
            let cc = col + dc;
            while rr >= grid.len() {
                grid.push(vec![false; num_cols]);
            }
            if cc < num_cols {
                grid[rr][cc] = true;
            }
        }
    }
}

fn resolve_tracks(
    tracks: &[GridTrack],
    container_size: f32,
    gap: f32,
    num_tracks: usize,
    child_sizes: &[(f32, f32)],
    placements: &[(usize, usize, usize, usize)],
    is_col: bool,
) -> Vec<f32> {
    let total_gaps = gap * (num_tracks as f32 - 1.0).max(0.0);
    let available = (container_size - total_gaps).max(0.0);

    let mut sizes = vec![0.0f32; num_tracks];
    let mut fr_total = 0.0f32;
    let mut fixed_total = 0.0f32;

    // First pass: Px and Auto
    for (i, track) in tracks.iter().enumerate() {
        if i >= num_tracks {
            break;
        }
        match track {
            GridTrack::Px(v) => {
                sizes[i] = *v;
                fixed_total += *v;
            }
            GridTrack::Auto => {
                let mut max_size = 0.0f32;
                for (ci, &(col, row, col_span, row_span)) in placements.iter().enumerate() {
                    let (track_start, span) = if is_col { (col, col_span) } else { (row, row_span) };
                    if track_start == i && span == 1 {
                        let s = if is_col {
                            child_sizes[ci].0
                        } else {
                            child_sizes[ci].1
                        };
                        max_size = max_size.max(s);
                    }
                }
                sizes[i] = max_size;
                fixed_total += max_size;
            }
            GridTrack::Fr(f) => {
                fr_total += f;
            }
        }
    }

    // Second pass: Fr tracks
    if fr_total > 0.0 {
        let fr_space = (available - fixed_total).max(0.0);
        for (i, track) in tracks.iter().enumerate() {
            if i >= num_tracks {
                break;
            }
            if let GridTrack::Fr(f) = track {
                sizes[i] = fr_space * f / fr_total;
            }
        }
    }

    sizes
}

fn align_item(item_size: f32, container_size: f32, align: &Align) -> (f32, Option<f32>) {
    match align {
        Align::Start => (0.0, None),
        Align::Center => ((container_size - item_size).max(0.0) / 2.0, None),
        Align::End => ((container_size - item_size).max(0.0), None),
        Align::Stretch => (0.0, Some(container_size)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::grid::Grid as GridComponent;
    use crate::components::shape::Shape;
    use crate::components::{ChildComponent, Component};
    use crate::schema::{GridPlacement, LayerStyle, ShapeType, Size};

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

    fn shape_child_placed(w: f32, h: f32, col: Option<i32>, row: Option<i32>) -> ChildComponent {
        let mut c = shape_child(w, h);
        if let Component::Shape(ref mut s) = c.component {
            if col.is_some() {
                s.style.grid_column = Some(GridPlacement {
                    start: col,
                    span: Some(1),
                });
            }
            if row.is_some() {
                s.style.grid_row = Some(GridPlacement {
                    start: row,
                    span: Some(1),
                });
            }
        }
        c
    }

    fn make_grid(children: Vec<ChildComponent>, cols: Vec<GridTrack>) -> GridComponent {
        let mut style = LayerStyle::default();
        style.grid_template_columns = Some(cols);
        GridComponent {
            children,
            size: None,
            animation: Default::default(),
            timing: Default::default(),
            style,
        }
    }

    #[test]
    fn test_grid_2_cols_fr() {
        let grid = make_grid(
            vec![shape_child(50.0, 50.0), shape_child(50.0, 50.0), shape_child(50.0, 50.0)],
            vec![GridTrack::Fr(1.0), GridTrack::Fr(1.0)],
        );
        let constraints = Constraints::tight(400.0, 300.0);
        let result = layout_grid(&grid, &constraints);

        assert_eq!(result.children.len(), 3);
        // 2 columns of 200px each (400/2)
        assert_eq!(result.children[0].x, 0.0);
        assert_eq!(result.children[0].y, 0.0);
        assert_eq!(result.children[1].x, 200.0);
        assert_eq!(result.children[1].y, 0.0);
        assert_eq!(result.children[2].x, 0.0);
        // Child 2 is on second row
    }

    #[test]
    fn test_grid_explicit_placement() {
        let grid = make_grid(
            vec![
                shape_child_placed(50.0, 50.0, Some(2), Some(1)),
                shape_child_placed(50.0, 50.0, Some(1), Some(1)),
            ],
            vec![GridTrack::Fr(1.0), GridTrack::Fr(1.0)],
        );
        let constraints = Constraints::tight(400.0, 300.0);
        let result = layout_grid(&grid, &constraints);

        // First child at col 2 (x=200), second at col 1 (x=0)
        assert_eq!(result.children[0].x, 200.0);
        assert_eq!(result.children[1].x, 0.0);
    }

    #[test]
    fn test_grid_with_gap() {
        let mut grid = make_grid(
            vec![shape_child(50.0, 50.0), shape_child(50.0, 50.0)],
            vec![GridTrack::Fr(1.0), GridTrack::Fr(1.0)],
        );
        grid.style.gap = Some(20.0);
        let constraints = Constraints::tight(420.0, 300.0);
        let result = layout_grid(&grid, &constraints);

        // Available = 420 - 20 gap = 400, each col = 200
        assert_eq!(result.children[0].x, 0.0);
        assert_eq!(result.children[1].x, 220.0); // 200 + 20 gap
    }
}
