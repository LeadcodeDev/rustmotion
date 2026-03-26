use crate::traits::{Align, GridConfig, GridTrack, LayoutItem};

use super::{Constraints, LayoutNode};

/// Measure a single layout item: widget measure + padding + margin.
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

/// Compute layout for a grid container's children.
pub fn layout_grid<L: LayoutItem>(
    children: &[L],
    style: &crate::schema::LayerStyle,
    constraints: &Constraints,
) -> LayoutNode {
    let config = GridConfig {
        grid_template_columns: style.grid_template_columns.clone(),
        grid_template_rows: style.grid_template_rows.clone(),
        gap: style.gap_or(0.0),
    };
    layout_grid_with_config(children, style, &config, constraints)
}

/// Compute layout for a container using the given grid config.
/// Useful for components like Card that manage grid config separately.
pub fn layout_grid_with_config<L: LayoutItem>(
    children: &[L],
    style: &crate::schema::LayerStyle,
    config: &GridConfig,
    constraints: &Constraints,
) -> LayoutNode {
    let n = children.len();
    if n == 0 {
        let (w, h) = constraints.constrain(0.0, 0.0);
        return LayoutNode::new(0.0, 0.0, w, h);
    }

    let styled = style;
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
    let row_tracks = config.grid_template_rows.as_deref().unwrap_or(&default_row);

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
        let gc = child.grid_column();
        let gr = child.grid_row();
        let col_start = gc.and_then(|g| g.start).map(|s| (s - 1).max(0) as usize);
        let row_start = gr.and_then(|g| g.start).map(|s| (s - 1).max(0) as usize);
        let col_span = gc.and_then(|g| g.span).unwrap_or(1).max(1) as usize;
        let row_span = gr.and_then(|g| g.span).unwrap_or(1).max(1) as usize;

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

    let content_w = col_offsets[num_cols] - config.gap + pl + pr;
    let content_h = row_offsets[actual_num_rows] - config.gap + pt + pb;
    // Use fixed size when constraints are tight (min == max), otherwise fit content.
    // This matches layout_flex behavior and allows auto-sized grid containers
    // to shrink-wrap their content (important for justify_content centering).
    let total_w = if constraints.min_width == constraints.max_width {
        constraints.max_width
    } else {
        content_w.min(constraints.max_width)
    };
    let total_h = if constraints.min_height == constraints.max_height {
        constraints.max_height
    } else {
        content_h.min(constraints.max_height)
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
                    let (track_start, span) = if is_col {
                        (col, col_span)
                    } else {
                        (row, row_span)
                    };
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
