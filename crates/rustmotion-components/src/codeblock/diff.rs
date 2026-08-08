use similar::{ChangeTag, TextDiff};
use skia_safe::{Canvas, Font, PaintStyle, Rect};
use syntect::highlighting::Theme;

use super::dimensions::lerp;
use super::highlight::highlight_code;
use super::reveal::{draw_line_number_at, draw_single_highlighted_line};
use super::Codeblock;
use rustmotion_core::engine::animator::ease;
use rustmotion_core::engine::renderer::paint_from_hex;
use rustmotion_core::schema::EasingType;

// ─── Types ───────────────────────────────────────────────────────────────────

pub(super) struct TransitionInfo {
    pub(super) code_a: String,
    pub(super) code_b: String,
    pub(super) progress: f64,
    #[allow(dead_code)]
    pub(super) easing: EasingType,
    pub(super) cursor_config: Option<rustmotion_core::schema::CodeblockCursor>,
}

#[derive(Debug)]
pub(super) enum LineDiffOp {
    Equal {
        #[allow(dead_code)]
        line: String,
        old_idx: usize,
        new_idx: usize,
    },
    Delete {
        #[allow(dead_code)]
        line: String,
        old_idx: usize,
    },
    Insert {
        #[allow(dead_code)]
        line: String,
        new_idx: usize,
    },
    Replace {
        old_line: String,
        new_line: String,
        old_idx: usize,
        new_idx: usize,
    },
}

#[derive(Debug)]
pub(super) struct FragmentEdit {
    col: usize,
    delete: String,
    insert: String,
}

// ─── State management ────────────────────────────────────────────────────────

pub(super) fn determine_active_state(
    layer: &Codeblock,
    time: f64,
) -> (String, Option<TransitionInfo>) {
    if layer.states.is_empty() {
        return (layer.code.clone(), None);
    }

    let mut current_code = layer.code.clone();

    for state in &layer.states {
        let end = state.at + state.duration;
        if time < state.at {
            return (current_code, None);
        } else if time < end {
            let raw_progress = (time - state.at) / state.duration;
            let progress = ease(raw_progress, &state.easing);
            return (
                current_code.clone(),
                Some(TransitionInfo {
                    code_a: current_code,
                    code_b: state.code.clone(),
                    progress,
                    easing: state.easing.clone(),
                    cursor_config: state.cursor.clone(),
                }),
            );
        } else {
            current_code = state.code.clone();
        }
    }

    (current_code, None)
}

// ─── Diff backgrounds ────────────────────────────────────────────────────────

/// Draw colored backgrounds for diff lines: green for `+`, red for `-`.
pub(super) fn draw_diff_backgrounds(
    canvas: &Canvas,
    code: &str,
    x: f32,
    y: f32,
    line_height: f32,
    width: f32,
    visible_lines: usize,
) {
    for (i, line) in code.lines().enumerate() {
        if i >= visible_lines {
            break;
        }
        let trimmed = line.trim_start();
        let bg_color = if trimmed.starts_with('+') {
            Some("#2D4F2D")
        } else if trimmed.starts_with('-') {
            Some("#4F2D2D")
        } else {
            None
        };

        if let Some(color) = bg_color {
            let mut paint = paint_from_hex(color);
            paint.set_anti_alias(false);
            let rect = Rect::from_xywh(x, y + i as f32 * line_height, width, line_height);
            canvas.draw_rect(rect, &paint);
        }
    }
}

// ─── Diff transition rendering ───────────────────────────────────────────────

/// Describes how to render a single line during a diff transition.
struct AnimatedLinePlacement {
    /// Interpolated Y position (in line-index space, multiply by line_height + add code_y later)
    old_y_idx: f32,
    new_y_idx: f32,
    /// Opacity at start (progress=0) and end (progress=1)
    opacity_start: f32,
    opacity_end: f32,
    /// Line numbers for old and new state (1-indexed)
    old_line_number: usize,
    new_line_number: usize,
    /// What kind of content to render
    content: AnimatedLineContent,
}

enum AnimatedLineContent {
    /// Render from highlighted_b at this index
    FromB { idx: usize },
    /// Render from highlighted_a at this index (for delete)
    FromA { idx: usize },
    /// Cursor-edited line (replace)
    CursorEdit {
        old_line: String,
        new_line: String,
        old_idx: usize,
        new_idx: usize,
    },
}

pub(super) fn render_diff_transition(
    canvas: &Canvas,
    layer: &Codeblock,
    font: &Font,
    theme: &Theme,
    code_x: f32,
    code_y: f32,
    line_height: f32,
    _gutter_width: f32,
    pad_left: f32,
    block_x: f32,
    trans: &TransitionInfo,
) {
    let progress = trans.progress as f32;
    let (_sw, metrics) = font.metrics();
    let ascent = -metrics.ascent;

    let diff_ops = compute_line_diff(&trans.code_a, &trans.code_b);
    let highlighted_a = highlight_code(&trans.code_a, &layer.language, theme);
    let highlighted_b = highlight_code(&trans.code_b, &layer.language, theme);

    let cursor_enabled = trans.cursor_config.as_ref().is_none_or(|c| c.enabled);
    let cursor_color = trans
        .cursor_config
        .as_ref()
        .map_or("#FFFFFF", |c| c.color.as_str());
    let cursor_width = trans.cursor_config.as_ref().map_or(2.0, |c| c.width);
    let cursor_blink = trans.cursor_config.as_ref().is_none_or(|c| c.blink);

    // Build animated line placements with proper interpolated positions.
    // Track "virtual cursors" for old and new index space so that
    // Insert/Delete lines get smooth starting/ending positions.
    let mut placements: Vec<AnimatedLinePlacement> = Vec::new();
    let mut _old_cursor: f32 = 0.0;
    let mut _new_cursor: f32 = 0.0;

    for op in &diff_ops {
        match op {
            LineDiffOp::Equal {
                old_idx, new_idx, ..
            } => {
                placements.push(AnimatedLinePlacement {
                    old_y_idx: *old_idx as f32,
                    new_y_idx: *new_idx as f32,
                    opacity_start: 1.0,
                    opacity_end: 1.0,
                    old_line_number: old_idx + 1,
                    new_line_number: new_idx + 1,
                    content: AnimatedLineContent::FromB { idx: *new_idx },
                });
                _old_cursor = *old_idx as f32 + 1.0;
                _new_cursor = *new_idx as f32 + 1.0;
            }
            LineDiffOp::Delete { old_idx, .. } => {
                // Line fades out at its old position (no Y movement)
                placements.push(AnimatedLinePlacement {
                    old_y_idx: *old_idx as f32,
                    new_y_idx: *old_idx as f32,
                    opacity_start: 1.0,
                    opacity_end: 0.0,
                    old_line_number: old_idx + 1,
                    new_line_number: old_idx + 1,
                    content: AnimatedLineContent::FromA { idx: *old_idx },
                });
                _old_cursor = *old_idx as f32 + 1.0;
                // _new_cursor does NOT advance for deletes
            }
            LineDiffOp::Insert { new_idx, .. } => {
                // Line fades in at its final position (no Y movement)
                placements.push(AnimatedLinePlacement {
                    old_y_idx: *new_idx as f32,
                    new_y_idx: *new_idx as f32,
                    opacity_start: 0.0,
                    opacity_end: 1.0,
                    old_line_number: new_idx + 1,
                    new_line_number: new_idx + 1,
                    content: AnimatedLineContent::FromB { idx: *new_idx },
                });
                _new_cursor = *new_idx as f32 + 1.0;
                // _old_cursor does NOT advance for inserts
            }
            LineDiffOp::Replace {
                old_line,
                new_line,
                old_idx,
                new_idx,
            } => {
                placements.push(AnimatedLinePlacement {
                    old_y_idx: *old_idx as f32,
                    new_y_idx: *new_idx as f32,
                    opacity_start: 1.0,
                    opacity_end: 1.0,
                    old_line_number: old_idx + 1,
                    new_line_number: new_idx + 1,
                    content: AnimatedLineContent::CursorEdit {
                        old_line: old_line.clone(),
                        new_line: new_line.clone(),
                        old_idx: *old_idx,
                        new_idx: *new_idx,
                    },
                });
                _old_cursor = *old_idx as f32 + 1.0;
                _new_cursor = *new_idx as f32 + 1.0;
            }
        }
    }

    // Pre-compute fragment edits for Replace ops
    let mut fragment_edits: Vec<(usize, usize, Vec<FragmentEdit>)> = Vec::new();
    for op in &diff_ops {
        if let LineDiffOp::Replace {
            old_line,
            new_line,
            old_idx,
            new_idx,
        } = op
        {
            let edits = compute_word_diff(old_line, new_line);
            fragment_edits.push((*old_idx, *new_idx, edits));
        }
    }

    // Render all placements
    let gutter_x = block_x + pad_left;

    for pl in &placements {
        let y_pos = code_y + lerp(pl.old_y_idx, pl.new_y_idx, progress) * line_height;
        let opacity = if pl.opacity_start == 0.0 && pl.opacity_end == 1.0 {
            // Inserted lines: wait until the expand animation is mostly done
            // before fading in the new code, to avoid overlapping text.
            let fade_start = 0.95;
            if progress < fade_start {
                0.0
            } else {
                (progress - fade_start) / (1.0 - fade_start)
            }
        } else {
            lerp(pl.opacity_start, pl.opacity_end, progress)
        };

        // Skip fully transparent lines
        if opacity < 0.005 {
            continue;
        }

        // Draw line number — use old number at start, new number at end
        if layer.show_line_numbers {
            let line_num = if progress < 0.5 {
                pl.old_line_number
            } else {
                pl.new_line_number
            };
            draw_line_number_at(canvas, font, gutter_x, y_pos + ascent, line_num, opacity);
        }

        // Draw content
        match &pl.content {
            AnimatedLineContent::FromB { idx } => {
                if let Some(line) = highlighted_b.get(*idx) {
                    draw_single_highlighted_line(
                        canvas,
                        line,
                        font,
                        code_x,
                        y_pos + ascent,
                        opacity,
                    );
                }
            }
            AnimatedLineContent::FromA { idx } => {
                if let Some(line) = highlighted_a.get(*idx) {
                    draw_single_highlighted_line(
                        canvas,
                        line,
                        font,
                        code_x,
                        y_pos + ascent,
                        opacity,
                    );
                }
            }
            AnimatedLineContent::CursorEdit {
                old_line,
                new_line,
                old_idx,
                new_idx,
            } => {
                if let Some((_oi, _ni, edits)) = fragment_edits
                    .iter()
                    .find(|(oi, ni, _)| oi == old_idx && ni == new_idx)
                {
                    draw_cursor_edited_line(
                        canvas,
                        font,
                        old_line,
                        new_line,
                        edits,
                        code_x,
                        y_pos + ascent,
                        trans.progress,
                        cursor_enabled,
                        cursor_color,
                        cursor_width,
                        cursor_blink,
                        &layer.language,
                        theme,
                    );
                }
            }
        }
    }
}

// ─── Line diff computation ───────────────────────────────────────────────────

pub(super) fn compute_line_diff(code_a: &str, code_b: &str) -> Vec<LineDiffOp> {
    let diff = TextDiff::from_lines(code_a, code_b);
    let mut ops = Vec::new();
    let mut old_idx = 0usize;
    let mut new_idx = 0usize;

    for change in diff.iter_all_changes() {
        let text = change.value().trim_end_matches('\n').to_string();
        match change.tag() {
            ChangeTag::Equal => {
                ops.push(LineDiffOp::Equal {
                    line: text,
                    old_idx,
                    new_idx,
                });
                old_idx += 1;
                new_idx += 1;
            }
            ChangeTag::Delete => {
                ops.push(LineDiffOp::Delete {
                    line: text,
                    old_idx,
                });
                old_idx += 1;
            }
            ChangeTag::Insert => {
                let merged = matches!(ops.last(), Some(LineDiffOp::Delete { .. }));
                if merged {
                    if let Some(LineDiffOp::Delete {
                        line: old_line,
                        old_idx: oi,
                    }) = ops.pop()
                    {
                        ops.push(LineDiffOp::Replace {
                            old_line,
                            new_line: text,
                            old_idx: oi,
                            new_idx,
                        });
                    }
                } else {
                    ops.push(LineDiffOp::Insert {
                        line: text,
                        new_idx,
                    });
                }
                new_idx += 1;
            }
        }
    }

    ops
}

// ─── Word-level diff for cursor animation ────────────────────────────────────

pub(super) fn compute_word_diff(old_line: &str, new_line: &str) -> Vec<FragmentEdit> {
    let diff = TextDiff::from_words(old_line, new_line);
    let mut edits = Vec::new();
    let mut col = 0usize;
    let mut pending_delete = String::new();

    for change in diff.iter_all_changes() {
        match change.tag() {
            ChangeTag::Equal => {
                if !pending_delete.is_empty() {
                    edits.push(FragmentEdit {
                        col,
                        delete: pending_delete.clone(),
                        insert: String::new(),
                    });
                    pending_delete.clear();
                }
                col += change.value().chars().count();
            }
            ChangeTag::Delete => {
                pending_delete.push_str(change.value());
            }
            ChangeTag::Insert => {
                edits.push(FragmentEdit {
                    col,
                    delete: pending_delete.clone(),
                    insert: change.value().to_string(),
                });
                pending_delete.clear();
                col += change.value().chars().count();
            }
        }
    }

    if !pending_delete.is_empty() {
        edits.push(FragmentEdit {
            col,
            delete: pending_delete,
            insert: String::new(),
        });
    }

    edits
}

// ─── Cursor-animated line editing ────────────────────────────────────────────

/// Byte offset of the `n`-th character, saturating at the end of the string.
///
/// Every column in a `FragmentEdit` is a character count, because the reveal
/// interpolates a *fraction* of the total edit length: counting bytes makes the
/// animation land part-way through a multi-byte glyph, which both slices at an
/// invalid boundary and spends three frames revealing one CJK character.
fn byte_at(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(i, _)| i)
        .unwrap_or(s.len())
}

pub(super) fn draw_cursor_edited_line(
    canvas: &Canvas,
    font: &Font,
    old_line: &str,
    new_line: &str,
    edits: &[FragmentEdit],
    x: f32,
    y: f32,
    progress: f64,
    cursor_enabled: bool,
    cursor_color: &str,
    cursor_width: f32,
    cursor_blink: bool,
    language: &str,
    theme: &Theme,
) {
    if edits.is_empty() {
        let highlighted = highlight_code(new_line, language, theme);
        if let Some(line) = highlighted.first() {
            draw_single_highlighted_line(canvas, line, font, x, y, 1.0);
        }
        return;
    }

    let total_work: usize = edits
        .iter()
        .map(|e| e.delete.chars().count() + e.insert.chars().count())
        .sum();
    if total_work == 0 {
        let highlighted = highlight_code(new_line, language, theme);
        if let Some(line) = highlighted.first() {
            draw_single_highlighted_line(canvas, line, font, x, y, 1.0);
        }
        return;
    }

    let chars_progress = (progress * total_work as f64).round() as usize;
    let mut current_line = old_line.to_string();
    let mut work_done = 0usize;
    let mut cursor_col: Option<usize> = None;
    let mut offset_adjust: i64 = 0;

    for edit in edits {
        let adjusted_col = (edit.col as i64 + offset_adjust).max(0) as usize;
        let delete_len = edit.delete.chars().count();
        let insert_len = edit.insert.chars().count();
        let edit_work = delete_len + insert_len;

        if work_done + edit_work <= chars_progress {
            let start = byte_at(&current_line, adjusted_col);
            let end = byte_at(&current_line, adjusted_col + delete_len);
            current_line.replace_range(start..end.max(start), &edit.insert);
            offset_adjust += insert_len as i64 - delete_len as i64;
            work_done += edit_work;
        } else {
            let remaining_progress = chars_progress - work_done;
            if remaining_progress < delete_len {
                let chars_deleted = remaining_progress;
                let first_deleted = adjusted_col + delete_len - chars_deleted;
                let del_start = byte_at(&current_line, first_deleted);
                let del_end = byte_at(&current_line, adjusted_col + delete_len);
                if del_start < del_end {
                    current_line.replace_range(del_start..del_end, "");
                }
                cursor_col = Some(first_deleted);
            } else {
                let chars_inserted = remaining_progress - delete_len;
                let start = byte_at(&current_line, adjusted_col);
                let end = byte_at(&current_line, adjusted_col + delete_len);
                let partial_insert = &edit.insert[..byte_at(&edit.insert, chars_inserted)];
                current_line.replace_range(start..end.max(start), partial_insert);
                cursor_col = Some(adjusted_col + chars_inserted);
            }
            break;
        }
    }

    let highlighted = highlight_code(&current_line, language, theme);
    if let Some(line) = highlighted.first() {
        draw_single_highlighted_line(canvas, line, font, x, y, 1.0);
    }

    if cursor_enabled {
        if let Some(col) = cursor_col {
            let should_show = if cursor_blink {
                let blink_time = progress * 10.0;
                (blink_time % 1.06).fract() < 0.53
            } else {
                true
            };

            if should_show {
                // `col` counts characters, like every other column here.
                let prefix = &current_line[..byte_at(&current_line, col)];
                let (prefix_width, _) = font.measure_str(prefix, None);
                let cursor_x = x + prefix_width;
                let mut cursor_paint = paint_from_hex(cursor_color);
                cursor_paint.set_style(PaintStyle::Fill);
                let (_sw, metrics) = font.metrics();
                let cursor_top = y - (-metrics.ascent);
                let cursor_bottom = y + metrics.descent;
                let cursor_rect = Rect::from_xywh(
                    cursor_x,
                    cursor_top,
                    cursor_width,
                    cursor_bottom - cursor_top,
                );
                canvas.draw_rect(cursor_rect, &cursor_paint);
            }
        }
    }
}
