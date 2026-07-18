# Studio Annotations (feedback → agent loop) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close the open-slide-style loop: select an element at a moment in the video, write a note ("describe a change for the agent"), and persist it in the scenario's `annotations` array. A panel lists pending annotations (jump-to + delete). A skill applies them: read each open annotation, make the change at its pointer, validate, mark resolved.

**Architecture:** The `Annotation` schema already exists (`crates/rustmotion-core/src/schema/scenario.rs`, from the engine-backbone plan). The studio builds an annotation JSON object from the current selection (pointer + kind, from the inspector) + current frame + view/scene (from the `FrameTask`), appends it to `raw["annotations"]`, and writes the file (the watcher reloads). A list panel reads `raw["annotations"]`. A `.claude/skills/apply-annotations` skill is the application mechanism, mirroring the existing `rustmotion` skill conventions.

**Tech Stack:** Rust + `serde_json` (studio), Dioxus (capture box + panel), a Markdown skill.

**Context for the engineer:**
- Branch `feat/rework-studio`. The studio model (`model.rs`) holds `raw: serde_json::Value` (the raw scenario JSON) and `path`. The watcher reloads on file change. `serde_json` has `preserve_order` enabled.
- Selection in `app_ui.rs` is `selected: Signal<Option<(u32 /*node_id*/, String /*pointer*/, String /*kind*/)>>`. The inspector renders when selected. `current: Signal<u32>` is the playhead. `tasks: Vec<FrameTask>` is in the model; `FrameTask::Normal { view_idx, scene_idx, .. }` gives the scene.
- The `Annotation` JSON shape (matches the schema): `{ "id": String, "note": String, "status": "open", "frame": u32, "view": usize, "scene": usize, "target": { "pointer": String, "kind": String } }`. `rect` under `target` is optional.
- Inspector edit helpers are in `crates/rustmotion-studio/src/preview/edit.rs`.
- **Re-render note:** controlled inputs in components that re-render on playback lose focus. Capture UI must live in a child component memoized by its props (see how `InspectorFields` does it in `app_ui.rs`), reading the playhead via a `Signal` prop only in the submit handler (not in render).
- Run tests: `cargo test -p rustmotion-studio`. Commit style `<type>: <verb> <message>`, NO Co-Authored-By / Claude mention.

---

### Task 1: Annotation JSON helpers + capture box

**Files:**
- Modify: `crates/rustmotion-studio/src/preview/edit.rs` (append/remove/list helpers + tests)
- Modify: `crates/rustmotion-studio/src/preview/app_ui.rs` (capture box child component, rendered in the inspector)

- [ ] **Step 1: Write the helpers + tests in `edit.rs`:**

```rust
/// Append an annotation object to `raw["annotations"]` (creating the array if
/// absent). Returns the mutated clone.
pub fn append_annotation(mut raw: Value, annotation: Value) -> Value {
    let arr = raw
        .as_object_mut()
        .map(|o| o.entry("annotations").or_insert_with(|| Value::Array(vec![])));
    if let Some(Value::Array(a)) = arr {
        a.push(annotation);
    }
    raw
}

/// Remove the annotation with the given id from `raw["annotations"]`.
pub fn remove_annotation(mut raw: Value, id: &str) -> Value {
    if let Some(Value::Array(a)) = raw.get_mut("annotations") {
        a.retain(|x| x.get("id").and_then(|v| v.as_str()) != Some(id));
    }
    raw
}

/// List the annotations as (id, note, frame, kind) tuples for the panel.
pub fn list_annotations(raw: &Value) -> Vec<(String, String, u64, String)> {
    raw.get("annotations")
        .and_then(|v| v.as_array())
        .map(|a| {
            a.iter()
                .map(|x| {
                    let id = x.get("id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let note = x.get("note").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let frame = x.get("frame").and_then(|v| v.as_u64()).unwrap_or(0);
                    let kind = x
                        .get("target")
                        .and_then(|t| t.get("kind"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    (id, note, frame, kind)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod annotation_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn append_then_list_then_remove() {
        let raw = json!({ "video": { "width": 1, "height": 1 }, "scenes": [] });
        let ann = json!({ "id": "an_1", "note": "smaller", "status": "open", "frame": 5,
            "target": { "pointer": "/scenes/0/children/0", "kind": "text" } });
        let raw = append_annotation(raw, ann);
        let list = list_annotations(&raw);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].0, "an_1");
        assert_eq!(list[0].1, "smaller");
        assert_eq!(list[0].2, 5);
        assert_eq!(list[0].3, "text");
        let raw = remove_annotation(raw, "an_1");
        assert!(list_annotations(&raw).is_empty());
    }
}
```
Run: `cargo test -p rustmotion-studio annotation_tests`. Expected: PASS.

- [ ] **Step 2: Add a capture box child component in `app_ui.rs`.** Memoized by props so it doesn't re-render on playback. Reads the playhead via a `Signal` prop only in the submit handler:

```rust
#[component]
fn AnnotationBox(pointer: String, kind: String, current: Signal<u32>) -> Element {
    let shared = use_context::<Shared>();
    let mut note = use_signal(String::new);

    let submit = move |_| {
        let text = note();
        if text.trim().is_empty() {
            return;
        }
        let (path, mut raw, view, scene) = {
            let m = shared.lock().unwrap();
            let frame = current();
            let (view, scene) = match m.tasks.get(frame as usize) {
                Some(rustmotion::encode::video::FrameTask::Normal { view_idx, scene_idx, .. }) => {
                    (*view_idx, *scene_idx)
                }
                _ => (0, 0),
            };
            (m.path.clone(), m.raw.clone(), view, scene)
        };
        let id = annotation_id();
        let ann = serde_json::json!({
            "id": id, "note": text, "status": "open", "frame": current(),
            "view": view, "scene": scene,
            "target": { "pointer": pointer, "kind": kind }
        });
        raw = super::edit::append_annotation(raw, ann);
        if let (Some(path), Ok(t)) = (path, serde_json::to_string_pretty(&raw)) {
            let _ = std::fs::write(&path, t);
            note.set(String::new());
        }
    };

    rsx! {
        div { style: "border-top:1px solid #1c1f27; padding-top:12px; display:flex; flex-direction:column; gap:8px;",
            div { style: "color:#7a7f8a;", "Leave a comment for the agent" }
            textarea {
                style: "min-height:64px; padding:6px; background:#0c0d10; color:#cfd3dc; border:1px solid #2a2f3a; resize:vertical;",
                value: "{note}",
                oninput: move |e| note.set(e.value()),
            }
            button {
                style: "padding:6px 10px; cursor:pointer; align-self:flex-end;",
                onclick: submit,
                "Add comment"
            }
        }
    }
}
```
Add a module-level helper for a unique id:
```rust
fn annotation_id() -> String {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("an_{n:x}")
}
```

- [ ] **Step 3: Render the capture box in the inspector panel.** In `StudioApp`, after the `InspectorFields { ... }` line (inside the inspector `div`), add (passing the playhead signal — note `current` is the existing `Signal<u32>`):
```rust
                    AnnotationBox { pointer: pointer.clone(), kind: selected_kind.clone().unwrap_or_default(), current }
```
(The inspector destructures `pointer`; clone it since it's used by `InspectorFields` too. `selected_kind` is `Option<String>`.)

- [ ] **Step 4: Build + test.** `cargo build -p rustmotion-studio`, `cargo test -p rustmotion-studio annotation_tests`. Expected: compiles, tests pass.

- [ ] **Step 5: VERIFY (HUMAN STEP).** Run the studio, select an element, type a comment, click "Add comment". Confirm `examples/component-showcase.json` gains an `annotations` array entry (`git diff`), and the textarea clears. Confirm it works while playing too.

- [ ] **Step 6: Commit.**
```bash
git add crates/rustmotion-studio/src/preview/edit.rs crates/rustmotion-studio/src/preview/app_ui.rs
git commit -m "feat: capture studio annotations into the scenario"
```

---

### Task 2: Annotations panel (list, delete, jump-to)

**Files:**
- Modify: `crates/rustmotion-studio/src/preview/app_ui.rs`

- [ ] **Step 1: Compute the list + a toggle.** In `StudioApp`, add `let mut show_annotations = use_signal(|| false);` near the other signals, and compute the list each render:
```rust
    let annotations = {
        let m = shared.lock().unwrap();
        super::edit::list_annotations(&m.raw)
    };
```

- [ ] **Step 2: Add a toggle button + count to the controls bar** (next to the frame counter):
```rust
                button {
                    style: "padding:6px 10px; cursor:pointer;",
                    onclick: move |_| show_annotations.toggle(),
                    "Comments ({annotations.len()})"
                }
```
(If `Signal::toggle` isn't available, use `show_annotations.set(!show_annotations())`.)

- [ ] **Step 3: Render the panel** (a left-side fixed panel, shown when `show_annotations()`):
```rust
            if show_annotations() {
                div { style: "position:fixed; top:0; left:0; width:280px; height:100vh; background:#13161d; border-right:1px solid #1c1f27; padding:16px; box-sizing:border-box; overflow:auto; display:flex; flex-direction:column; gap:10px;",
                    div { style: "color:#4c8dff; font-weight:600;", "Comments" }
                    if annotations.is_empty() {
                        div { style: "color:#7a7f8a;", "No comments yet." }
                    }
                    for (id, note, frame, kind) in annotations.iter().cloned() {
                        div { style: "border:1px solid #2a2f3a; border-radius:6px; padding:8px; display:flex; flex-direction:column; gap:6px;",
                            div { style: "color:#7a7f8a; font-size:11px;", "{kind} · frame {frame}" }
                            div { "{note}" }
                            div { style: "display:flex; gap:8px;",
                                button {
                                    style: "cursor:pointer; padding:2px 8px;",
                                    onclick: move |_| current.set(frame as u32),
                                    "Go to frame"
                                }
                                button {
                                    style: "cursor:pointer; padding:2px 8px; color:#ff6b6b;",
                                    onclick: {
                                        let shared = shared.clone();
                                        let id = id.clone();
                                        move |_| delete_annotation(&shared, &id)
                                    },
                                    "Delete"
                                }
                            }
                        }
                    }
                }
            }
```
Add the module-level helper:
```rust
fn delete_annotation(shared: &Shared, id: &str) {
    let (path, raw) = {
        let m = shared.lock().unwrap();
        (m.path.clone(), m.raw.clone())
    };
    let raw = super::edit::remove_annotation(raw, id);
    if let (Some(path), Ok(t)) = (path, serde_json::to_string_pretty(&raw)) {
        let _ = std::fs::write(&path, t);
    }
}
```

- [ ] **Step 4: Build.** `cargo build -p rustmotion-studio`. Expected: compiles.

- [ ] **Step 5: VERIFY (HUMAN STEP).** Add a couple of comments, open the Comments panel, confirm they list with kind + frame; "Go to frame" seeks; "Delete" removes it from the panel and the file.

- [ ] **Step 6: Commit.**
```bash
git add crates/rustmotion-studio/src/preview/app_ui.rs
git commit -m "feat: add studio annotations panel with delete and seek"
```

---

### Task 3: The `apply-annotations` skill

A Markdown skill that applies open annotations and marks them resolved.

**Files:**
- Create: `.claude/skills/apply-annotations/SKILL.md`

- [ ] **Step 1: Write the skill.** Create `.claude/skills/apply-annotations/SKILL.md`:

```markdown
---
name: apply-annotations
description: Apply pending studio annotations from a Rustmotion scenario — for each open annotation, make the requested change at its target element, validate, and mark it resolved.
---

# Apply Annotations

Use this when a Rustmotion scenario has `annotations` (created in the studio) to apply.

## Process

1. Read the scenario file's `annotations` array. For each entry with `status: "open"`:
   - `target.pointer` is an RFC 6901 JSON Pointer to the element (e.g. `/scenes/2/children/5`).
   - `note` is the change request; `frame`/`view`/`scene` give the moment; `target.kind` is the component type.
2. For each open annotation, **apply the requested change** by editing the element at `target.pointer` (typically its `style`), interpreting `note`. Make the smallest edit that satisfies the note.
3. After each edit, run `rustmotion validate -f <file>` (schema + geometry). Both passes must succeed; if geometry fails, adjust (e.g. keep `wrap: true`) and re-validate.
4. Mark the annotation `status: "resolved"` (do not delete — the studio/`validate --fix` can strip resolved ones later).
5. Report a short summary: which annotations were applied and what changed.

## Rules

- Apply exactly what each `note` asks — do not make unrequested changes.
- Never break the `validate` passes; the scenario must stay renderable.
- Preserve the rest of the JSON (key order, other elements).
- If a `note` is ambiguous, make a reasonable minimal interpretation and say so in the summary.
```

- [ ] **Step 2: Commit.**
```bash
git add .claude/skills/apply-annotations/SKILL.md
git commit -m "feat: add apply-annotations skill"
```

---

## Self-Review

**Spec coverage** (`docs/superpowers/specs/2026-06-09-studio-v2-dioxus-design.md`, sub-project E):
- "capture : clic élément + point temporel + note → append `annotations`" → Task 1. ✓
- "panneau : liste, suppression, marqueurs/seek" → Task 2. ✓
- "application : skill apply-then-validate, marque resolved" → Task 3. ✓
- Persisted in the scenario `annotations` field (the schema from the backbone plan). ✓
- Rides on the existing selection + pointer. ✓

**Scope limits:** annotation `target.rect` is omitted (optional; pointer+kind suffice for the skill). Editing during playback works because the capture box is a memoized child reading the playhead only at submit. The reload-loop on write is tolerated (annotations don't affect rendering; a hash-guard to skip re-render on annotation-only changes is a later optimization). `status: resolved` annotations are kept (strippable later) rather than deleted by the skill.

**Placeholder scan:** none — every code step is complete. The id helper, submit handler, panel, and skill body are all concrete.

**Type consistency:** `append_annotation(Value, Value) -> Value`, `remove_annotation(Value, &str) -> Value`, `list_annotations(&Value) -> Vec<(String,String,u64,String)>`, `AnnotationBox(pointer, kind, current: Signal<u32>)`, `delete_annotation(&Shared, &str)`, `annotation_id() -> String` — consistent across tasks.

## Hand-off note

This closes the loop the user set out to build: studio feedback (open-slide style) → persisted annotations → a skill applies them. The studio is now a full select → inspect → edit → annotate authoring tool, with Skia as the only video engine.
