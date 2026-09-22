//! Regression tests — audit round chantier/audit-2026-09, workstream E
//! (animated backgrounds & the scene render path).
//!
//! Two of the four findings are bugs in fully private
//! rendering internals (`background.rs`'s `tile_spacing`/`compute_scroll_offset`/
//! `draw_bg_heropattern`) that this crate never exposes past its `pub`
//! surface — an external integration test crate like this one cannot name
//! them. Their regression tests live as `#[cfg(test)]` modules inside
//! `crates/rustmotion/src/engine/render/background.rs` itself, following
//! that file's own pre-existing convention (`scroll_offset_wrap_tests`,
//! `pixel_grid_tests`, `grid_lines_tests`, `halo_opacity_tests`) for testing
//! renderer-private logic directly. This file carries the findings that are
//! genuinely reachable through the crate's public API.
//!
//! One section per finding: the paint path, then colour templating.

use rustmotion::encode::video::{build_frame_tasks, render_frame_task, FrameTask};
use rustmotion::loader::load_scenario_from_source;
