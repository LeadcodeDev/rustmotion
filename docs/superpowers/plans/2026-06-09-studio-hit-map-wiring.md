# Studio Real Hit-Map Wiring Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the studio's hardcoded hotspot with **real, clickable element hotspots** derived from the engine's per-frame hit-map: every component painted in the current frame becomes a clickable overlay box (in percentage coords), and clicking one selects it and shows its type.

**Architecture:** The engine already emits a `HitMap` (`paint_tree_with_hits`, Plan 1) at the single `paint_tree` call site in `engine/render/scene.rs:332`, where `built.components` maps each `NodeId` back to its source `ChildComponent`. We enrich each hit there into `{rect, kind, node_id}` and bubble it up via an optional out-parameter through the existing render chain, exposing a new top-level `render_frame_task_scaled_with_hits`. The studio computes the current frame's hits (cheap — render only, no JPEG encode), converts rects to percentages, and renders one overlay box per hit; clicking selects by `node_id`.

**Tech Stack:** Rust, the existing Skia render pipeline, Dioxus (studio overlay).

**Scope / deferral:** This plan covers **selection of real elements** (hotspots + highlight + show kind). It uses `node_id` as the selection key (stable within a rendered frame). The durable **JSON Pointer** for an element and **property editing** (the inspector) are the NEXT plan — they need source-path tracking in the box builder, deliberately not added here. Only `FrameTask::Normal` frames get hits; transitions/world frames return an empty hit-map for now (note it, don't fake it).

**Context for the engineer:**
- We are on branch `feat/rework-studio`.
- The hit-map primitives exist: `crates/rustmotion-core/src/engine/paint_pass.rs` has `HitRect { x,y,w,h }`, `HitNode { node_id: NodeId, rect: HitRect }`, `type HitMap = Vec<HitNode>`, and `paint_tree_with_hits(canvas, root, layout, frame, dispatcher) -> HitMap`.
- The paint call site is `crates/rustmotion/src/engine/render/scene.rs:315-332` (function `render_with_new_pipeline_iter`). `built: BuiltScene` (`built.root`, `built.components: Vec<Option<&ChildComponent>>`) and `layout` are in scope there.
- The render chain (each returns `Result<Vec<u8>>` RGBA): `render_frame_task_scaled` (`crates/rustmotion/src/encode/video/tasks.rs:46`) → `render_scene_frame_scaled_with_prev_bg` (`scene.rs:444`) → `render_frame_v2_scaled` (`scene.rs:52`) → `render_with_new_pipeline` (`scene.rs:277`) → `render_with_new_pipeline_iter` (`scene.rs:297`).
- `ChildComponent.component` is a `Component` enum; `Component` variants are listed in `crates/rustmotion-components/src/box_builder.rs::component_style` (one arm per component).
- The studio renders frames via `frames::render_frame` and `render_frame_task_scaled`; it has `model.scenario`, `model.tasks: Vec<FrameTask>`, and a `current` frame signal.
- Run tests: `cargo test -p rustmotion-core -p rustmotion-components -p rustmotion`. Build studio: `cargo build -p rustmotion-studio`.
- Commit style `<type>: <verb> <message>`, NO Co-Authored-By / Claude mention.

---

### Task 1: `component_kind` helper + `EnrichedHit` type

A pure mapping from a component to a short kind label, and a data type carrying an on-screen hit with its kind.

**Files:**
- Modify: `crates/rustmotion-core/src/engine/paint_pass.rs` (add `EnrichedHit`)
- Modify: `crates/rustmotion-components/src/box_builder.rs` (add `component_kind`)

- [ ] **Step 1: Add `EnrichedHit` to core (failing test first).** In `crates/rustmotion-core/src/engine/paint_pass.rs`, after the `HitMap` type, add:

```rust
/// A hit enriched with a human/agent-facing component kind label, ready for the
/// studio overlay. `node_id` is stable within a single rendered frame.
#[derive(Debug, Clone, PartialEq)]
pub struct EnrichedHit {
    pub node_id: crate::engine::box_tree::NodeId,
    pub kind: String,
    pub rect: HitRect,
}
```
(No test needed — it's a plain data struct exercised by Task 2/3.)

- [ ] **Step 2: Write the failing test for `component_kind`.** In `crates/rustmotion-components/src/box_builder.rs`, add at the end of the `#[cfg(test)] mod tests`:

```rust
    #[test]
    fn component_kind_labels() {
        use crate::text::Text;
        let t = Component::Text(Text {
            content: "x".into(),
            max_width: None,
            timing: Default::default(),
            style: Default::default(),
            timeline: Vec::new(),
            stagger: None,
            text_shadow: None,
            stroke: None,
            text_background: None,
        });
        assert_eq!(component_kind(&t), "text");
    }
```

- [ ] **Step 3: Run it to see it fail.** `cargo test -p rustmotion-components component_kind_labels` → FAIL (`component_kind` not found).

- [ ] **Step 4: Implement `component_kind`.** In `crates/rustmotion-components/src/box_builder.rs`, add a public function next to `component_style`. Mirror its match arms, returning a kebab/lowercase label per variant:

```rust
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
```

- [ ] **Step 5: Run it to pass.** `cargo test -p rustmotion-components component_kind_labels` → PASS. Then `cargo build -p rustmotion-components`.

- [ ] **Step 6: Commit.**
```bash
git add crates/rustmotion-core/src/engine/paint_pass.rs crates/rustmotion-components/src/box_builder.rs
git commit -m "feat: add component_kind helper and EnrichedHit type"
```

---

### Task 2: Bubble enriched hits out of the render path

Thread an optional `hits` out-parameter from the `paint_tree` call site up to a new public `render_frame_task_scaled_with_hits`. Existing render entry points keep their exact behavior (pass `None`).

**Files:**
- Modify: `crates/rustmotion/src/engine/render/scene.rs` (functions at lines 297, 277, 52, 444)
- Modify: `crates/rustmotion/src/encode/video/tasks.rs` (add `render_frame_task_scaled_with_hits`)

- [ ] **Step 1: Enrich + collect at the paint site.** In `crates/rustmotion/src/engine/render/scene.rs`, change the signature of `render_with_new_pipeline_iter` to take a trailing `hits_out: Option<&mut Vec<EnrichedHit>>`, and replace the `paint_tree(...)` call (line ~332) with:

```rust
    if let Some(out) = hits_out {
        let hits = paint_tree_with_hits(canvas, &built.root, &layout, &frame, &dispatcher);
        out.reserve(hits.len());
        for h in hits {
            // built.components[node_id] is the source ChildComponent (None for
            // synthetic containers — skip those).
            if let Some(Some(child)) = built.components.get(h.node_id as usize) {
                out.push(EnrichedHit {
                    node_id: h.node_id,
                    kind: component_kind(&child.component).to_string(),
                    rect: h.rect,
                });
            }
        }
    } else {
        paint_tree(canvas, &built.root, &layout, &frame, &dispatcher);
    }
```
Add the needed imports to `scene.rs`: `use rustmotion_core::engine::paint_pass::{paint_tree_with_hits, EnrichedHit};` (next to the existing `paint_tree` import) and `use rustmotion_components::box_builder::component_kind;` (match the crate path already used for `build_scene_from_refs`).

- [ ] **Step 2: Thread the param through the 3 wrapper functions.** Add a trailing `hits_out: Option<&mut Vec<EnrichedHit>>` parameter to `render_with_new_pipeline` (line 277), `render_frame_v2_scaled` (line 52), and `render_scene_frame_scaled_with_prev_bg` (line 444), passing it straight down to the next call. At each call site that previously called these, pass `None` (the compiler will list them — update every one to add the `None` argument). `EnrichedHit` import is already added in Step 1.

- [ ] **Step 3: Add the public hits entry point.** In `crates/rustmotion/src/encode/video/tasks.rs`, add next to `render_frame_task_scaled`:

```rust
use rustmotion_core::engine::paint_pass::EnrichedHit;

/// Like [`render_frame_task_scaled`] but also returns the per-frame enriched
/// hit-map (component bounding boxes in video pixels). Only `Normal` frames
/// produce hits; transitions/world frames return an empty Vec.
pub fn render_frame_task_scaled_with_hits(
    config: &VideoConfig,
    scenario: &Scenario,
    task: &FrameTask,
    scale_factor: f32,
) -> Result<(Vec<u8>, Vec<EnrichedHit>)> {
    use crate::engine::render::render_scene_frame_scaled_with_prev_bg;
    match task {
        FrameTask::Normal { view_idx, scene_idx, frame_in_scene, scene_total_frames } => {
            let view = &scenario.views[*view_idx];
            let scene = &view.scenes[*scene_idx];
            let prev_bg = if *scene_idx > 0 {
                let prev = &view.scenes[*scene_idx - 1];
                Some((&prev.resolved_background, prev.duration))
            } else {
                None
            };
            let mut hits = Vec::new();
            let rgba = render_scene_frame_scaled_with_prev_bg(
                config, scene, *frame_in_scene, *scene_total_frames, scale_factor, prev_bg,
                Some(&mut hits),
            )?;
            Ok((rgba, hits))
        }
        // Transitions / world frames: render normally, no hits for now.
        other => Ok((render_frame_task_scaled(config, scenario, other, scale_factor)?, Vec::new())),
    }
}
```
Re-export it where `render_frame_task_scaled` is exported (find that `pub use` / `pub fn` in `crates/rustmotion/src/encode/video/mod.rs` or `crates/rustmotion/src/encode/mod.rs` and add `render_frame_task_scaled_with_hits` alongside).

- [ ] **Step 4: Build + regression test.** `cargo build -p rustmotion` then `cargo test -p rustmotion-core -p rustmotion-components -p rustmotion`. Expected: compiles (all `None` call sites updated), existing tests pass — the `None` path is byte-for-byte the old behavior.

- [ ] **Step 5: Add a smoke test.** In `crates/rustmotion/src/encode/video/tasks.rs` (or the nearest test module), add:

```rust
#[cfg(test)]
mod hit_tests {
    use super::*;

    const SCENARIO: &str = r##"{
        "video": { "width": 800, "height": 600, "background": "#101418" },
        "scenes": [ { "duration": 1.0, "children": [
            { "type": "text", "content": "Hello", "x": 100, "y": 80, "style": { "font-size": 48 } }
        ] } ]
    }"##;

    #[test]
    fn normal_frame_returns_text_hit() {
        let scenario = crate::loader::load_scenario_from_source(None, Some(SCENARIO)).unwrap();
        let tasks = crate::encode::build_frame_tasks(&scenario);
        let (rgba, hits) = render_frame_task_scaled_with_hits(&scenario.video, &scenario, &tasks[0], 1.0).unwrap();
        assert!(!rgba.is_empty());
        assert!(hits.iter().any(|h| h.kind == "text"), "expected a text hit, got {hits:?}");
    }
}
```
Run: `cargo test -p rustmotion normal_frame_returns_text_hit`. Expected: PASS. (If the inline `text` child JSON shape is rejected by the loader, adjust the child JSON to the schema — check `ChildComponent`/`Component` serde in `crates/rustmotion-components/src/lib.rs` — but keep the assertion that a `text` hit appears.)

- [ ] **Step 6: Commit.**
```bash
git add crates/rustmotion/src/engine/render/scene.rs crates/rustmotion/src/encode/video/tasks.rs crates/rustmotion/src/encode
git commit -m "feat: expose per-frame enriched hit-map from render path"
```

---

### Task 3: Real element hotspots in the studio overlay

The studio computes the current frame's hits (render only, no encode — cheap), converts to percentages, renders one overlay box per element, and selects on click.

**Files:**
- Modify: `crates/rustmotion-studio/src/preview/frames.rs` (add `frame_hits`)
- Modify: `crates/rustmotion-studio/src/preview/app_ui.rs` (replace hardcoded hotspot)

- [ ] **Step 1: Add `frame_hits` (percentage-space) with a test.** In `crates/rustmotion-studio/src/preview/frames.rs`:

```rust
/// A clickable element box in percentage-of-frame coords, with its kind.
#[derive(Debug, Clone, PartialEq)]
pub struct HitPct {
    pub node_id: u32,
    pub kind: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Compute the current frame's clickable element boxes in percentage coords.
pub fn frame_hits(
    scenario: &ResolvedScenario,
    tasks: &[rustmotion::encode::video::FrameTask],
    frame: u32,
) -> Vec<HitPct> {
    let idx = (frame as usize).min(tasks.len().saturating_sub(1));
    let task = &tasks[idx];
    let vw = scenario.video.width as f32;
    let vh = scenario.video.height as f32;
    match rustmotion::encode::render_frame_task_scaled_with_hits(&scenario.video, scenario, task, 1.0) {
        Ok((_rgba, hits)) => hits
            .into_iter()
            .map(|h| HitPct {
                node_id: h.node_id,
                kind: h.kind,
                x: (h.rect.x / vw) * 100.0,
                y: (h.rect.y / vh) * 100.0,
                w: (h.rect.w / vw) * 100.0,
                h: (h.rect.h / vh) * 100.0,
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

#[cfg(test)]
mod hit_pct_tests {
    use super::*;
    const SCENARIO: &str = r##"{ "video": { "width": 800, "height": 600, "background": "#101418" }, "scenes": [ { "duration": 1.0, "children": [ { "type": "text", "content": "Hi", "x": 80, "y": 60, "style": { "font-size": 40 } } ] } ] }"##;

    #[test]
    fn frame_hits_are_in_percent_and_have_kind() {
        let scenario = rustmotion::loader::load_scenario_from_source(None, Some(SCENARIO)).unwrap();
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        let hits = frame_hits(&scenario, &tasks, 0);
        assert!(hits.iter().any(|h| h.kind == "text"));
        assert!(hits.iter().all(|h| h.x >= 0.0 && h.x <= 100.0 && h.w <= 100.0));
    }
}
```
Run: `cargo test -p rustmotion-studio frame_hits_are_in_percent`. Expected: PASS. (Adjust the child JSON shape if the loader rejects it, as in Task 2 Step 5.)

- [ ] **Step 2: Render real hotspots in `app_ui.rs`.** Remove the hardcoded `hotspot_border` / `hotspot_style` block and the single hardcoded overlay `div`. Replace `selected: Signal<bool>` with `selected: Signal<Option<u32>>` (the selected `node_id`). Compute hits for the current frame and store them:

```rust
    let mut selected = use_signal(|| None::<u32>);
    // Recompute hits whenever the frame changes (render only, ~ms; no encode).
    let hits = {
        let m = shared.lock().unwrap();
        super::frames::frame_hits(&m.scenario, &m.tasks, cur)
    };
```
Then render the overlay as one box per hit (replace the overlay `div`'s inner content):

```rust
                    div { style: "position:absolute; inset:0;",
                        for hit in hits.iter() {
                            div {
                                key: "{hit.node_id}",
                                style: format!(
                                    "position:absolute; left:{}%; top:{}%; width:{}%; height:{}%; box-sizing:border-box; cursor:pointer; border:{};",
                                    hit.x, hit.y, hit.w, hit.h,
                                    if selected() == Some(hit.node_id) { "2px solid #4c8dff; background:rgba(76,141,255,0.15)" }
                                    else { "1px dashed rgba(255,255,255,0.25)" }
                                ),
                                onclick: {
                                    let id = hit.node_id;
                                    move |_| selected.set(Some(id))
                                },
                            }
                        }
                    }
```
And add a small label showing the selected element's kind (near the controls):

```rust
                if let Some(id) = selected() {
                    if let Some(h) = hits.iter().find(|h| h.node_id == id) {
                        div { style: "min-width:140px; color:#4c8dff;", "selected: {h.kind}" }
                    }
                }
```

- [ ] **Step 3: Build.** `cargo build -p rustmotion-studio`. Expected: compiles.

- [ ] **Step 4: VERIFY (HUMAN STEP).** Run `cargo run -p rustmotion-studio -- -f examples/component-showcase.json`. Confirm: dashed boxes appear around real elements, they track the elements as you scrub/play (boxes move with animation), clicking a box highlights it solid-blue and shows `selected: <kind>`. Boxes stay aligned on window resize.

- [ ] **Step 5: Commit.**
```bash
git add crates/rustmotion-studio/src/preview/frames.rs crates/rustmotion-studio/src/preview/app_ui.rs
git commit -m "feat: overlay real element hotspots from the engine hit-map"
```

---

## Self-Review

**Spec coverage** (`docs/superpowers/specs/2026-06-09-studio-v2-dioxus-design.md`, sub-project C + the overlay model):
- "hit-map émis par le paint_pass … rect + kind par nœud" → Task 1 (`component_kind`, `EnrichedHit`) + Task 2 (enrich at paint site). ✓
- "Dioxus pose des hotspots transparents … en % des dimensions vidéo, alignement automatique" → Task 3 (`frame_hits` → `%`, overlay boxes). ✓
- "clic → sélectionne l'élément" → Task 3 (select by `node_id`, highlight, show kind). ✓
- Engine stays the single source of geometry (hits come from the same build+layout+paint as the frame) → Task 2 reuses the real render path. ✓

**Intentional deferral:** JSON Pointer (`target.pointer`) and property editing (inspector) are NOT here — they require source-path tracking in `box_builder` (NodeId→child-index path) and a property write-back layer. That is the next plan; this one selects by `node_id` (valid within a rendered frame) and shows kind. `FrameTask` transitions/world frames return empty hits (noted, not faked).

**Placeholder scan:** none — every code step has complete code. The only adjust-if-needed note is the inline test scenario's `text` child JSON shape (verify against the loader); the assertions stand regardless.

**Type consistency:** `EnrichedHit { node_id: NodeId, kind: String, rect: HitRect }`, `render_frame_task_scaled_with_hits(...) -> Result<(Vec<u8>, Vec<EnrichedHit>)>`, `HitPct { node_id, kind, x, y, w, h }`, `component_kind(&Component) -> &'static str`, `selected: Signal<Option<u32>>` — used identically across tasks.

**Perf note:** `frame_hits` runs the render (≈2 ms at 1080p) but NOT the JPEG encode, so per-frame overhead is small. If it ever shows up, unify with the asset handler via a one-render-per-frame cache (model keyed by frame) — out of scope here.

## Hand-off note

After this lands, selection works on real elements. The next plan adds source-path/JSON-Pointer tracking + the property inspector (edit → write JSON → reload) and the annotation panel, both riding on this selection.
