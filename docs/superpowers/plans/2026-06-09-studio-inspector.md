# Studio Inspector (JSON Pointer + property editing) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give each selectable element a durable **JSON Pointer** into the source scenario, then add an **inspector**: select an element → see its key style props → edit them → the change is written back to the scenario JSON file → the existing watcher hot-reloads the preview.

**Architecture:** The box builder learns each node's source path (relative `"/children/{i}/children/{j}"`) and stores it on `BoxNode`. The engine hit-map (`render_scene_hits`) reads that path; the studio combines it with the current scene's document prefix (`/scenes/{s}` or `/composition/{v}/scenes/{s}`, derived from the raw JSON) to form a full JSON Pointer. The inspector reads the element's `style` object from the raw scenario JSON via that pointer, renders editable fields for a starter set of properties, and on edit rewrites the file with `serde_json` + `Value::pointer_mut`. The watcher reload path (already built) refreshes the frame.

**Tech Stack:** Rust, `serde_json` (`Value::pointer` / `pointer_mut`), Dioxus (inspector panel), the existing studio watcher.

**Scope:** Editable props v1 = `color` (text), `font-size`, `background` (all under the element's `style` object). More props later. Write-back targets the element's `style` map. Only `FrameTask::Normal` frames have editable selections (consistent with the hit-map). Container vs leaf doesn't matter — any element with a `style` object can be edited.

**Context for the engineer:**
- Branch `feat/rework-studio`. The hit-map already works: `rustmotion::encode::render_frame_task_hits(scenario, task) -> Vec<EnrichedHit { node_id, kind, rect }>` (in `crates/rustmotion/src/encode/video/tasks.rs`), backed by `engine::render::render_scene_hits` (`crates/rustmotion/src/engine/render/scene.rs`). The studio's `frames::frame_hits` converts to `HitPct { node_id, kind, x, y, w, h }` and the overlay selects by `node_id`.
- `BoxNode` is in `crates/rustmotion-core/src/engine/box_tree.rs`: `{ id: NodeId, kind: BoxKind, css: CssStyle, children: Vec<BoxNode>, intrinsic: Option<Arc<dyn IntrinsicMeasure>> }`, with constructors `container(css, children)` and `leaf(css, intrinsic)` and a `find(&self, id) -> Option<&BoxNode>`.
- The box builder is `crates/rustmotion-components/src/box_builder.rs`: `build_scene_from_refs` builds the root, `build_child` builds each node (assigns `id`, pushes to `components`), `container_children` recurses into Card/Flex/Grid/Container/Positioned children.
- `EnrichedHit` is in `crates/rustmotion-core/src/engine/paint_pass.rs`.
- The studio model (`crates/rustmotion-studio/src/preview/model.rs`) holds `scenario: ResolvedScenario`, `tasks: Vec<FrameTask>`; the watcher in `mod.rs` reloads on file change and bumps `generation`. `input_path` (the scenario file path) is available in `run_preview_inner` but NOT currently stored in the model — Task 3 adds it.
- Run tests: `cargo test -p rustmotion-core -p rustmotion-components -p rustmotion -p rustmotion-studio`. Note: `rustmotion`'s `all_components_deserialize` is a PRE-EXISTING unrelated failure (`container` variant) — ignore it; don't let it mask your task's tests (run your tests by name).
- Commit style `<type>: <verb> <message>`, NO Co-Authored-By / Claude mention.

---

### Task 1: `source_path` on `BoxNode`, threaded through the box builder

Each node gets a relative JSON path from the scene root.

**Files:**
- Modify: `crates/rustmotion-core/src/engine/box_tree.rs` (add field + constructors)
- Modify: `crates/rustmotion-components/src/box_builder.rs` (thread the path)
- Modify: `crates/rustmotion-core/src/engine/paint_pass.rs` (update the 2 test `BoxNode` literals in `hit_tests`)

- [ ] **Step 1: Add the field to `BoxNode`.** In `box_tree.rs`, add `pub source_path: Option<String>` to the struct (after `intrinsic`), and set it in both constructors:

```rust
pub struct BoxNode {
    pub id: NodeId,
    pub kind: BoxKind,
    pub css: CssStyle,
    pub children: Vec<BoxNode>,
    pub intrinsic: Option<Arc<dyn IntrinsicMeasure>>,
    /// JSON path of this node relative to its scene's `children` array, e.g.
    /// "/children/2/children/0". `None` for synthetic nodes (the scene root).
    pub source_path: Option<String>,
}

impl BoxNode {
    pub fn container(css: CssStyle, children: Vec<BoxNode>) -> Self {
        Self { id: 0, kind: BoxKind::Container, css, children, intrinsic: None, source_path: None }
    }

    pub fn leaf(css: CssStyle, intrinsic: Arc<dyn IntrinsicMeasure>) -> Self {
        Self { id: 0, kind: BoxKind::Container, css, children: Vec::new(), intrinsic: Some(intrinsic), source_path: None }
    }
    // ... keep assign_ids / find unchanged ...
}
```

- [ ] **Step 2: Thread the path in `box_builder.rs`.** Change `build_child` to take a `path: String`, set `source_path: Some(path.clone())` on the node it builds, and pass child paths into `container_children`:
  - In `build_scene_from_refs`, replace the children loop:
    ```rust
    let mut child_boxes = Vec::new();
    for (i, c) in children.into_iter().enumerate() {
        child_boxes.push(build_child(c, &mut components, &mut next_id, anim, format!("/children/{i}")));
    }
    ```
  - The root `BoxNode { id: 0, kind: BoxKind::Container, css: root_css, children: child_boxes, intrinsic: None }` literal gains `source_path: None`.
  - `build_child(child, components, next_id, anim, path: String)`: change the returned literal to include `source_path: Some(path.clone())`, and call `let children_boxes = container_children(&child.component, components, next_id, anim, &path);`.
  - `container_children(component, components, next_id, anim, parent_path: &str)`: replace the `.map(|c| build_child(...))` with an enumerated version:
    ```rust
    children
        .iter()
        .enumerate()
        .map(|(j, c)| build_child(c, components, next_id, anim, format!("{parent_path}/children/{j}")))
        .collect()
    ```

- [ ] **Step 3: Fix the `BoxNode` literals in `paint_pass.rs` tests.** In `crates/rustmotion-core/src/engine/paint_pass.rs`, the `hit_tests` module constructs `BoxNode { ... }` literals (the leaf and root in each test). Add `source_path: None,` to each (there are several — the compiler will point at every one).

- [ ] **Step 4: Write a test for the path.** In `box_builder.rs` `#[cfg(test)] mod tests`, add:

```rust
    #[test]
    fn build_child_records_source_path() {
        // A card with two shapes — nested children get nested paths.
        let card = make_card(
            vec![make_shape(10.0, 10.0), make_shape(10.0, 10.0)],
            CssStyle::default(),
        );
        let scene = vec![ChildComponent {
            component: card,
            position: Some(crate::PositionMode::Absolute { x: 0.0, y: 0.0 }),
            x: None,
            y: None,
            z_index: None,
        }];
        let built = build_scene(&scene, (800.0, 600.0));
        let card_box = &built.root.children[0];
        assert_eq!(card_box.source_path.as_deref(), Some("/children/0"));
        assert_eq!(card_box.children[1].source_path.as_deref(), Some("/children/0/children/1"));
    }
```

- [ ] **Step 5: Build + test.** `cargo build -p rustmotion-core -p rustmotion-components` then `cargo test -p rustmotion-components build_child_records_source_path` and `cargo test -p rustmotion-core hit_tests`. Expected: PASS.

- [ ] **Step 6: Commit.**
```bash
git add crates/rustmotion-core/src/engine/box_tree.rs crates/rustmotion-components/src/box_builder.rs crates/rustmotion-core/src/engine/paint_pass.rs
git commit -m "feat: track source json path on box nodes"
```

---

### Task 2: Carry the pointer through the hit-map to the studio

Add `pointer` to `EnrichedHit`, fill it from the node's `source_path`, and have the studio combine it with the scene's document prefix into a full JSON Pointer.

**Files:**
- Modify: `crates/rustmotion-core/src/engine/paint_pass.rs` (`EnrichedHit` gains `pointer`)
- Modify: `crates/rustmotion/src/engine/render/scene.rs` (`render_scene_hits` fills `pointer`)
- Modify: `crates/rustmotion-studio/src/preview/frames.rs` (`HitPct` gains `pointer`; build full pointer)

- [ ] **Step 1: Add `pointer` to `EnrichedHit`.** In `paint_pass.rs`:
```rust
pub struct EnrichedHit {
    pub node_id: NodeId,
    pub kind: String,
    pub rect: HitRect,
    /// JSON path relative to the scene's `children`, e.g. "/children/2".
    pub pointer: Option<String>,
}
```

- [ ] **Step 2: Fill it in `render_scene_hits`.** In `scene.rs`, in the enrichment closure, look the node up to read its `source_path`:
```rust
    hits.into_iter()
        .filter_map(|h| {
            let child = built.components.get(h.node_id as usize).copied().flatten()?;
            let pointer = built.root.find(h.node_id).and_then(|n| n.source_path.clone());
            Some(EnrichedHit {
                node_id: h.node_id,
                kind: component_kind(&child.component).to_string(),
                rect: h.rect,
                pointer,
            })
        })
        .collect()
```
(The smoke test `normal_frame_returns_text_hit` still passes — it only checks `kind`.)

- [ ] **Step 3: Build the full pointer in the studio.** In `crates/rustmotion-studio/src/preview/frames.rs`, add `pub pointer: Option<String>` to `HitPct`. Change `frame_hits` to accept the scene document prefix and prepend it:
```rust
pub fn frame_hits(
    scenario: &ResolvedScenario,
    tasks: &[rustmotion::encode::video::FrameTask],
    frame: u32,
    scene_prefix: &str,
) -> Vec<HitPct> {
    if tasks.is_empty() {
        return Vec::new();
    }
    let idx = (frame as usize).min(tasks.len() - 1);
    let task = &tasks[idx];
    let vw = scenario.video.width as f32;
    let vh = scenario.video.height as f32;
    rustmotion::encode::render_frame_task_hits(scenario, task)
        .into_iter()
        .map(|h| HitPct {
            node_id: h.node_id,
            kind: h.kind,
            x: (h.rect.x / vw) * 100.0,
            y: (h.rect.y / vh) * 100.0,
            w: (h.rect.w / vw) * 100.0,
            h: (h.rect.h / vh) * 100.0,
            pointer: h.pointer.map(|rel| format!("{scene_prefix}{rel}")),
        })
        .collect()
}
```
Update the existing `frame_hits_are_in_percent_and_have_kind` test to pass a prefix (e.g. `"/scenes/0"`) and additionally assert the text hit's `pointer` starts with `"/scenes/0/children/"`.

- [ ] **Step 4: Add a helper to derive the scene prefix from the raw JSON + current task.** In `frames.rs`:
```rust
/// JSON-Pointer prefix to the scene of the given frame, derived from the raw
/// scenario JSON (handles both top-level `scenes` and `composition`).
pub fn scene_prefix(
    raw: &serde_json::Value,
    tasks: &[rustmotion::encode::video::FrameTask],
    frame: u32,
) -> String {
    use rustmotion::encode::video::FrameTask;
    if tasks.is_empty() {
        return String::new();
    }
    let idx = (frame as usize).min(tasks.len() - 1);
    if let FrameTask::Normal { view_idx, scene_idx, .. } = &tasks[idx] {
        if raw.get("composition").is_some() {
            format!("/composition/{view_idx}/scenes/{scene_idx}")
        } else {
            format!("/scenes/{scene_idx}")
        }
    } else {
        String::new()
    }
}
```
Add a unit test that builds a `serde_json::json!` with top-level `scenes` and asserts the prefix is `"/scenes/0"`.

- [ ] **Step 5: Build + test.** `cargo build -p rustmotion-studio` and `cargo test -p rustmotion-studio frame_hits` and the new prefix test. Expected: PASS.

- [ ] **Step 6: Commit.**
```bash
git add crates/rustmotion-core/src/engine/paint_pass.rs crates/rustmotion/src/engine/render/scene.rs crates/rustmotion-studio/src/preview/frames.rs
git commit -m "feat: carry full json pointer through the studio hit-map"
```

---

### Task 3: Inspector panel — read + edit style props, write back to JSON

Store the scenario file path + raw JSON in the model; render an inspector for the selected element; on edit, rewrite the file (the watcher reloads).

**Files:**
- Modify: `crates/rustmotion-studio/src/preview/model.rs` (store `path` + raw JSON)
- Modify: `crates/rustmotion-studio/src/preview/mod.rs` (populate them)
- Create: `crates/rustmotion-studio/src/preview/edit.rs` (read/write a style prop — unit tested)
- Modify: `crates/rustmotion-studio/src/preview/app_ui.rs` (inspector panel)

- [ ] **Step 1: Write the edit helpers with tests.** Create `crates/rustmotion-studio/src/preview/edit.rs`:

```rust
use serde_json::Value;

/// Read a style property string at `pointer`'s element, e.g. ("/scenes/0/children/0", "color").
pub fn read_style(raw: &Value, pointer: &str, prop: &str) -> Option<String> {
    let el = raw.pointer(pointer)?;
    let v = el.get("style")?.get(prop)?;
    Some(match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    })
}

/// Set a style property (as a JSON string value) on the element at `pointer`.
/// Returns the mutated clone; caller writes it to disk.
pub fn set_style(mut raw: Value, pointer: &str, prop: &str, value: &str) -> Option<Value> {
    let el = raw.pointer_mut(pointer)?;
    if !el.is_object() {
        return None;
    }
    let obj = el.as_object_mut()?;
    let style = obj
        .entry("style")
        .or_insert_with(|| Value::Object(Default::default()));
    let style_obj = style.as_object_mut()?;
    style_obj.insert(prop.to_string(), Value::String(value.to_string()));
    Some(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn raw() -> Value {
        json!({ "video": { "width": 1, "height": 1 },
            "scenes": [ { "duration": 1.0, "children": [
                { "type": "text", "content": "Hi", "style": { "color": "#fff" } }
            ] } ] })
    }

    #[test]
    fn reads_existing_style_prop() {
        assert_eq!(read_style(&raw(), "/scenes/0/children/0", "color").as_deref(), Some("#fff"));
    }

    #[test]
    fn sets_and_reads_back_a_prop() {
        let updated = set_style(raw(), "/scenes/0/children/0", "color", "#ff0000").unwrap();
        assert_eq!(read_style(&updated, "/scenes/0/children/0", "color").as_deref(), Some("#ff0000"));
    }

    #[test]
    fn creates_style_object_when_absent() {
        let mut r = raw();
        r["scenes"][0]["children"][0].as_object_mut().unwrap().remove("style");
        let updated = set_style(r, "/scenes/0/children/0", "font-size", "48").unwrap();
        assert_eq!(read_style(&updated, "/scenes/0/children/0", "font-size").as_deref(), Some("48"));
    }
}
```
Run: `cargo test -p rustmotion-studio --lib edit`. Expected: 3 PASS. Add `mod edit;` to `mod.rs`.

- [ ] **Step 2: Store the path + raw JSON in the model.** In `model.rs`, add to `StudioModel`: `pub path: Option<std::path::PathBuf>` and `pub raw: serde_json::Value`. In `StudioModel::new`, accept them (or load `raw` from `path`). Simplest: add params:
```rust
    pub fn new(scenario: ResolvedScenario, error: Option<String>, path: Option<std::path::PathBuf>) -> Self {
        let raw = path
            .as_ref()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(serde_json::Value::Null);
        let tasks = rustmotion::encode::build_frame_tasks(&scenario);
        let total_frames = tasks.len() as u32;
        Self { scenario, tasks, total_frames, error, generation: 0, path, raw }
    }
```
Update both `StudioModel::new(...)` call sites in `mod.rs` (`run_preview_inner` passes `input_path.clone()`; the watcher's reload passes `Some(path.clone())`).

- [ ] **Step 3: Inspector panel in `app_ui.rs`.** Compute `scene_prefix` once (`let prefix = { let m = shared.lock().unwrap(); super::frames::scene_prefix(&m.raw, &m.tasks, cur) };`) and pass it to `frame_hits`. When an element is selected, find its `HitPct` to get `pointer`; read its current `color`/`font-size`/`background` from `m.raw` via `edit::read_style`; render labeled `<input>`s. On input change, compute the new raw via `edit::set_style`, write the file, and let the watcher reload:

```rust
    let apply = move |pointer: String, prop: &'static str, value: String| {
        let (path, raw) = {
            let m = shared.lock().unwrap();
            (m.path.clone(), m.raw.clone())
        };
        if let (Some(path), Some(updated)) = (path, super::edit::set_style(raw, &pointer, prop, &value)) {
            if let Ok(text) = serde_json::to_string_pretty(&updated) {
                let _ = std::fs::write(&path, text);
                // The file watcher reloads the model (and bumps generation),
                // which refreshes the frame and the in-memory `raw`.
            }
        }
    };
```
Render the panel only when `selected_kind` is `Some` and the selected hit has a `pointer`. For each of `color`, `font-size`, `background`, show `input { value: "{current}", onchange: move |e| apply(pointer.clone(), prop, e.value()) }`. (`onchange` fires on commit, avoiding a write per keystroke.) Clone `apply`/`pointer` per field as needed (Dioxus event handlers are `FnMut`).

- [ ] **Step 4: Build.** `cargo build -p rustmotion-studio`. Expected: compiles.

- [ ] **Step 5: VERIFY (HUMAN STEP).** `cargo run -p rustmotion-studio -- -f examples/component-showcase.json`. Select a text element → the inspector shows its color/size; change the color value and commit (Tab/Enter) → the file is rewritten and the preview reloads with the new color within ~250 ms. Confirm undo works by editing the file back, and that selecting different elements shows their own values.

- [ ] **Step 6: Commit.**
```bash
git add crates/rustmotion-studio/src/preview/edit.rs crates/rustmotion-studio/src/preview/model.rs crates/rustmotion-studio/src/preview/mod.rs crates/rustmotion-studio/src/preview/app_ui.rs
git commit -m "feat: add studio property inspector with json write-back"
```

---

## Self-Review

**Spec coverage** (`docs/superpowers/specs/2026-06-09-studio-v2-dioxus-design.md`, sub-project D + E foundation):
- "inspecteur : sélection → éditer props → write-back JSON → reload" → Tasks 2+3. ✓
- "JSON Pointer (RFC 6901) vers l'élément dans le JSON source" → Tasks 1+2 (source_path → full pointer). ✓
- Element reference now durable (a JSON Pointer, not just `node_id`) — unblocks the annotation panel too. ✓
- Single source of geometry preserved (pointer comes from the same build as the rendered frame). ✓

**Intentional scope limits:** editable props are `color`/`font-size`/`background` only (the write-back machinery is general — adding props is one more field each). Pointer handles `scenes` and `composition`; deeply nested `$ref`/config-resolved children may not map 1:1 to the raw file (note for later). Annotation panel is a separate follow-up that reuses the pointer.

**Placeholder scan:** none — every code step is complete. The inspector field wiring (Task 3 Step 3) is described with the exact `apply` closure + per-field pattern; the engineer instantiates it for 3 props.

**Type consistency:** `BoxNode.source_path: Option<String>`, `EnrichedHit { node_id, kind, rect, pointer: Option<String> }`, `HitPct { …, pointer: Option<String> }`, `frame_hits(scenario, tasks, frame, scene_prefix)`, `scene_prefix(raw, tasks, frame) -> String`, `read_style(raw, pointer, prop) -> Option<String>`, `set_style(raw, pointer, prop, value) -> Option<Value>` — consistent across tasks.

## Hand-off note

After this lands, the studio is a real editor: select → inspect → edit → see it. The annotation panel (the original open-slide-style feedback loop) is the next plan and rides entirely on this pointer + selection.
