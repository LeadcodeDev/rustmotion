# Engine Backbone (Hit-map + Annotations) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add the renderer-agnostic backbone the Studio v2 needs: a per-frame hit-map emitted by the paint pass (so clicks can map to elements) and an `annotations` field on the scenario schema (so feedback persists in the file).

**Architecture:** Both changes live in `rustmotion-core` and are independent of any GUI (winit, Dioxus, Blitz). The paint pass already walks the box tree top-down computing each node's on-screen position; we make it optionally collect each component node's device-space bounding box keyed by its layout `NodeId`. The scenario gains an `annotations: Vec<Annotation>` field that the renderer never reads and that serializes only when non-empty. Mapping `NodeId → JSON Pointer + component kind` and converting rects to viewport-percentages are deliberately deferred to the studio-side frame-service (a later plan) where component types and scene context are available.

**Tech Stack:** Rust, `skia-safe` (raster surface + canvas matrix), `taffy` (via the existing `run_layout`), `serde` / `serde_json`, `schemars` (`JsonSchema`).

**Context for the engineer:**
- Workspace crates: `rustmotion-core` (engine + schema), `rustmotion-components` (the 51 components), `rustmotion-cli`.
- Run a single test: `cargo test -p rustmotion-core <test_name>`.
- Run a module's tests: `cargo test -p rustmotion-core <mod_name>`.
- Commit style: Conventional Commits (`feat:` / `test:` / `refactor:`). Do **not** add `Co-Authored-By` trailers.

---

### Task 1: `annotations` schema field

Add an `annotations` array to `Scenario`, plus the `Annotation` / `AnnotationTarget` / `AnnotationStatus` types. The field defaults to empty and is skipped on serialization when empty, so production scenarios stay clean and the renderer is unaffected (it never reads the field).

**Files:**
- Modify: `crates/rustmotion-core/src/schema/scenario.rs` (add types; add one field to `struct Scenario`, lines 35-55)
- Test: `crates/rustmotion-core/src/schema/scenario.rs` (new `#[cfg(test)] mod annotation_tests` at end of file)

- [ ] **Step 1: Write the failing tests**

Append to the end of `crates/rustmotion-core/src/schema/scenario.rs`:

```rust
#[cfg(test)]
mod annotation_tests {
    use super::*;

    const MINIMAL: &str = r#"{ "video": { "width": 1920, "height": 1080 }, "scenes": [] }"#;

    const WITH_ANNOTATIONS: &str = r#"{
        "video": { "width": 1920, "height": 1080 },
        "scenes": [],
        "annotations": [
            {
                "id": "an_4f2a",
                "note": "reduce font-size",
                "status": "open",
                "frame": 142,
                "view": 0,
                "scene": 2,
                "target": {
                    "pointer": "/scenes/2/children/5",
                    "kind": "text",
                    "rect": [10.0, 20.0, 30.0, 40.0]
                }
            }
        ]
    }"#;

    #[test]
    fn scenario_without_annotations_defaults_empty() {
        let s: Scenario = serde_json::from_str(MINIMAL).unwrap();
        assert!(s.annotations.is_empty());
    }

    #[test]
    fn scenario_with_annotations_deserializes() {
        let s: Scenario = serde_json::from_str(WITH_ANNOTATIONS).unwrap();
        assert_eq!(s.annotations.len(), 1);
        let a = &s.annotations[0];
        assert_eq!(a.id, "an_4f2a");
        assert_eq!(a.status, AnnotationStatus::Open);
        assert_eq!(a.frame, Some(142));
        assert_eq!(a.view, Some(0));
        assert_eq!(a.scene, Some(2));
        assert_eq!(a.target.pointer, "/scenes/2/children/5");
        assert_eq!(a.target.kind.as_deref(), Some("text"));
        assert_eq!(a.target.rect, Some([10.0, 20.0, 30.0, 40.0]));
    }

    #[test]
    fn empty_annotations_are_not_serialized() {
        let s: Scenario = serde_json::from_str(MINIMAL).unwrap();
        let json = serde_json::to_string(&s).unwrap();
        assert!(
            !json.contains("annotations"),
            "empty annotations must be skipped, got: {json}"
        );
    }

    #[test]
    fn status_defaults_to_open_and_target_fields_optional() {
        let json = r#"{
            "video": { "width": 1, "height": 1 },
            "scenes": [],
            "annotations": [ { "id": "x", "note": "n", "target": { "pointer": "/scenes/0" } } ]
        }"#;
        let s: Scenario = serde_json::from_str(json).unwrap();
        assert_eq!(s.annotations[0].status, AnnotationStatus::Open);
        assert_eq!(s.annotations[0].target.kind, None);
        assert_eq!(s.annotations[0].target.rect, None);
        assert_eq!(s.annotations[0].frame, None);
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test -p rustmotion-core annotation_tests`
Expected: FAIL to compile — `no field 'annotations' on type 'Scenario'` and `cannot find type 'Annotation'/'AnnotationStatus'`.

- [ ] **Step 3: Add the field and the types**

In `crates/rustmotion-core/src/schema/scenario.rs`, add the new field inside `struct Scenario` (right after the `backgrounds` field, before the closing `}` at line 54-55):

```rust
    /// Studio feedback annotations. Persisted in the scenario but never read by
    /// the renderer or the geometry validator. Skipped on serialization when empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<Annotation>,
```

Then add the type definitions immediately after the `Scenario` struct's closing brace (after line 55):

```rust
/// Lifecycle of a studio annotation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum AnnotationStatus {
    Open,
    Resolved,
}

impl Default for AnnotationStatus {
    fn default() -> Self {
        AnnotationStatus::Open
    }
}

/// What an annotation points at: a JSON Pointer into the source scenario,
/// plus optional context captured at click time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct AnnotationTarget {
    /// RFC 6901 JSON Pointer into the source scenario (e.g. "/scenes/2/children/5").
    pub pointer: String,
    /// Component kind label captured at click time (e.g. "text", "card").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Bounding box [x, y, w, h] in video coords at the capture frame.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rect: Option<[f32; 4]>,
}

/// A single studio feedback note attached to an element at a moment in time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Annotation {
    /// Stable id (generated by the studio).
    pub id: String,
    /// Free-text change request for the agent/skill.
    pub note: String,
    #[serde(default)]
    pub status: AnnotationStatus,
    /// Frame index at capture time (global playhead).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub frame: Option<u32>,
    /// Resolved view index (convenience for the skill).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<usize>,
    /// Resolved scene index (convenience for the skill).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene: Option<usize>,
    pub target: AnnotationTarget,
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test -p rustmotion-core annotation_tests`
Expected: PASS (4 tests).

- [ ] **Step 5: Commit**

```bash
git add crates/rustmotion-core/src/schema/scenario.rs
git commit -m "feat: add annotations field to scenario schema"
```

---

### Task 2: Per-frame hit-map in the paint pass

Make the paint pass optionally collect, for every component-backed node, its on-screen (device-space) bounding box keyed by layout `NodeId`. Video rendering uses the existing `paint_tree` (no collection, zero overhead); the studio will call the new `paint_tree_with_hits`.

**Files:**
- Modify: `crates/rustmotion-core/src/engine/paint_pass.rs` (add types, add a field to `PaintContext`, add `paint_tree_with_hits`, emit in `paint_node`)
- Test: `crates/rustmotion-core/src/engine/paint_pass.rs` (new `#[cfg(test)] mod hit_tests` at end of file)

- [ ] **Step 1: Write the failing test**

Append to the end of `crates/rustmotion-core/src/engine/paint_pass.rs`:

```rust
#[cfg(test)]
mod hit_tests {
    use super::*;
    use std::sync::Arc;

    use crate::css::style::{CssStyle, Display, FlexDirection, Position, Size as CSize};
    use crate::css::taffy_bridge::ConversionContext;
    use crate::css::units::LengthPercentage as CLP;
    use crate::engine::box_tree::{BoxKind, BoxNode};
    use crate::engine::layout_pass::run_layout;

    fn test_frame(w: u32, h: u32) -> PaintFrame {
        PaintFrame {
            time: 0.0,
            frame_index: 0,
            fps: 30,
            video_width: w,
            video_height: h,
            scene_duration: 1.0,
        }
    }

    #[test]
    fn hitmap_reports_component_rect_for_untransformed_node() {
        // A component leaf, absolutely positioned at (40, 30), sized 100x80,
        // inside a flex-column root that fills a 400x400 viewport.
        let leaf = BoxNode {
            id: 0,
            kind: BoxKind::Component(Arc::new(1u32)),
            css: CssStyle {
                position: Some(Position::Absolute),
                left: Some(CLP::Px(40.0)),
                top: Some(CLP::Px(30.0)),
                width: Some(CSize::Length(CLP::Px(100.0))),
                height: Some(CSize::Length(CLP::Px(80.0))),
                ..Default::default()
            },
            children: vec![],
            intrinsic: None,
        };
        let mut root = BoxNode {
            id: 0,
            kind: BoxKind::Container,
            css: CssStyle {
                display: Some(Display::Flex),
                flex_direction: Some(FlexDirection::Column),
                width: Some(CSize::Length(CLP::Px(400.0))),
                height: Some(CSize::Length(CLP::Px(400.0))),
                ..Default::default()
            },
            children: vec![leaf],
            intrinsic: None,
        };
        root.assign_ids(0);

        let layout = run_layout(&root, (400.0, 400.0), &ConversionContext::default());
        let mut surface = skia_safe::surfaces::raster_n32_premul((400, 400)).unwrap();
        let hits = paint_tree_with_hits(
            surface.canvas(),
            &root,
            &layout,
            &test_frame(400, 400),
            &NoopDispatcher,
        );

        // Only the component leaf is reported, not the synthetic container root.
        assert_eq!(hits.len(), 1, "expected exactly one component hit");
        let h = &hits[0];
        assert_eq!(h.node_id, root.children[0].id);
        assert!((h.rect.x - 40.0).abs() < 0.5, "x = {}", h.rect.x);
        assert!((h.rect.y - 30.0).abs() < 0.5, "y = {}", h.rect.y);
        assert!((h.rect.w - 100.0).abs() < 0.5, "w = {}", h.rect.w);
        assert!((h.rect.h - 80.0).abs() < 0.5, "h = {}", h.rect.h);
    }
}
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `cargo test -p rustmotion-core hit_tests`
Expected: FAIL to compile — `cannot find function 'paint_tree_with_hits'` and `cannot find type 'HitNode'`.

- [ ] **Step 3: Add the hit-map types**

In `crates/rustmotion-core/src/engine/paint_pass.rs`, add `use std::cell::RefCell;` to the top imports (after line 16's `use skia_safe::{...}` block), then add these types right after the `PaintFrame` struct (after line 42):

```rust
/// Axis-aligned bounding box of a painted node, in device (video-pixel) coords.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitRect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// One clickable node: its layout id and on-screen bounding box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HitNode {
    pub node_id: crate::engine::box_tree::NodeId,
    pub rect: HitRect,
}

/// Hit-test map for a single painted frame, in paint order (so later entries
/// are visually on top).
pub type HitMap = Vec<HitNode>;
```

- [ ] **Step 4: Add the `hits` collector to `PaintContext` and the new entry point**

Replace the `PaintContext` struct (lines 93-98) with:

```rust
struct PaintContext<'a> {
    layout: &'a LayoutResult,
    frame: &'a PaintFrame,
    dispatcher: &'a dyn PaintDispatcher,
    viewport_size: (f32, f32),
    hits: Option<&'a RefCell<HitMap>>,
}
```

Replace the body of `paint_tree` (lines 77-91) with a version that passes `hits: None`, and add `paint_tree_with_hits` right after it:

```rust
/// Paint a fully-laid-out box tree onto a Skia canvas.
pub fn paint_tree(
    canvas: &Canvas,
    root: &BoxNode,
    layout: &LayoutResult,
    frame: &PaintFrame,
    dispatcher: &dyn PaintDispatcher,
) {
    let ctx = PaintContext {
        layout,
        frame,
        dispatcher,
        viewport_size: (frame.video_width as f32, frame.video_height as f32),
        hits: None,
    };
    paint_node(canvas, root, &ctx);
}

/// Like [`paint_tree`] but also returns the per-frame hit-map: the on-screen
/// bounding box of every component-backed node, in paint order. Used by the
/// studio for click-to-select; the video render path uses [`paint_tree`].
pub fn paint_tree_with_hits(
    canvas: &Canvas,
    root: &BoxNode,
    layout: &LayoutResult,
    frame: &PaintFrame,
    dispatcher: &dyn PaintDispatcher,
) -> HitMap {
    let hits = RefCell::new(Vec::new());
    let ctx = PaintContext {
        layout,
        frame,
        dispatcher,
        viewport_size: (frame.video_width as f32, frame.video_height as f32),
        hits: Some(&hits),
    };
    paint_node(canvas, root, &ctx);
    hits.into_inner()
}
```

- [ ] **Step 5: Emit a hit for each component node**

In `paint_node`, immediately after the transform block (after line 125's closing `}` of the `if node.css.transform.is_some() ...` block, before the `// 3. opacity layer` comment), insert:

```rust
    // Hit-map: record the on-screen bbox of component-backed nodes. The canvas
    // matrix here already includes this node's and all ancestors' transforms,
    // so mapping the (absolute) layout rect yields the device-space AABB.
    if let (Some(hits), BoxKind::Component(_)) = (ctx.hits, &node.kind) {
        let local = Rect::from_xywh(
            box_layout.x,
            box_layout.y,
            box_layout.width,
            box_layout.height,
        );
        let dev = canvas.total_matrix().map_rect(local).0;
        hits.borrow_mut().push(HitNode {
            node_id: node.id,
            rect: HitRect {
                x: dev.left,
                y: dev.top,
                w: dev.width(),
                h: dev.height(),
            },
        });
    }
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `cargo test -p rustmotion-core hit_tests`
Expected: PASS (1 test).

- [ ] **Step 7: Run the full core test suite (no regressions)**

Run: `cargo test -p rustmotion-core`
Expected: PASS, including the pre-existing paint-pass `tests` module and the new `hit_tests`.

- [ ] **Step 8: Commit**

```bash
git add crates/rustmotion-core/src/engine/paint_pass.rs
git commit -m "feat: emit per-frame hit-map from paint pass"
```

---

## Self-Review

**Spec coverage** (against `docs/superpowers/specs/2026-06-09-studio-v2-dioxus-design.md`, section "Fondation (sous-projet B)" + "North star ... E"):

- "schema : champ top-level `annotations` ... ignoré au rendu et par la passe geometry de `validate`" → Task 1. The renderer/validator never read the field; `skip_serializing_if` keeps production JSON clean. ✓
- "`paint_pass` émet un `HitMap` pour la frame courante ... rect en coords vidéo, ordre de peinture = z-order" → Task 2. ✓ (emission happens during the same top-down walk; paint order preserved.)
- Annotation data shape `{ id, note, status, frame, view, scene, target: { pointer, kind, rect } }` → Task 1 types match. ✓

**Intentional refinements vs the spec** (documented so the next plan picks them up):
- The spec put `source_path` on `BoxNode` and `pointer`/`kind` on the hit node. This plan keeps the core hit node minimal (`node_id` + device `rect`) and **defers** `NodeId → JSON Pointer + kind` resolution to the studio-side frame-service, where the `BuiltScene.components` lookup table and scene index are available. This avoids touching every `BoxNode` construction site and keeps `paint_pass` free of component-type knowledge (preserving the existing dispatcher decoupling). The follow-up plan (frame-service) must: map each `HitNode.node_id` to its `ChildComponent` via `BuiltScene.components`, derive the component kind label, build the JSON Pointer from child indices, and convert `rect` (video px) → viewport-percent for the Dioxus overlay.

**Placeholder scan:** none — every code step contains complete code; every run step has an exact command and expected outcome.

**Type consistency:** `HitNode { node_id, rect }`, `HitRect { x, y, w, h }`, `HitMap = Vec<HitNode>`, `paint_tree_with_hits(...) -> HitMap`, `AnnotationStatus::{Open,Resolved}`, `AnnotationTarget { pointer, kind, rect }`, `Annotation { id, note, status, frame, view, scene, target }` — names used identically in tests and implementations.

## Follow-up plans (not in this plan)

- **Plan 2 — Dioxus frame-transport spike** (gates the studio): boot a `dioxus-desktop` window, register `use_asset_handler("frame", …)`, display a Skia frame via `<img src="/frame/…">`, measure encode + transport + sustained fps at 1180×2256 and 1920×1080 against the thresholds in the spec.
- **Plan 3 — Studio foundation**: frame-service (NodeId→pointer/kind enrichment, rect→% conversion), canvas + clickable overlay, playback/timeline, watch→signal. Built only after the spike validates the 0.7 API.
