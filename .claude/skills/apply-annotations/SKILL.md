---
name: apply-annotations
description: Apply pending studio annotations from a Rustmotion scenario — for each open annotation, make the requested change at its target element, validate, and mark it resolved.
---

# Apply Annotations

Use this when a Rustmotion scenario has an `annotations` array (created in the studio's "Leave a comment for the agent" box) to apply.

## Process

1. Read the scenario file's `annotations` array. For each entry with `status: "open"`:
   - `target.pointer` is an RFC 6901 JSON Pointer to the element (e.g. `/scenes/2/children/5`).
   - `note` is the change request.
   - `frame` / `view` / `scene` give the moment in the video; `target.kind` is the component type (`text`, `card`, …).
2. For each open annotation, **apply the requested change** by editing the element at `target.pointer` — usually a property under its `style` object — interpreting `note`. Make the smallest edit that satisfies the note.
3. After each edit, run `rustmotion validate -f <file>` (schema + geometry). Both passes must succeed. If geometry fails (e.g. `unwrappable_text_overflow`), adjust (keep `wrap: true`, lower a `font-size`, …) and re-validate.
4. Set the annotation's `status` to `"resolved"` (do not delete it — the studio panel and `validate --fix` can strip resolved ones later).
5. Report a short summary: which annotations were applied and what changed.

## Rules

- Apply exactly what each `note` asks — no unrequested changes.
- Never break the `validate` passes; the scenario must stay renderable.
- Preserve the rest of the JSON (key order, other elements, the `annotations` array).
- Resolve the pointer against the **raw** scenario file (it addresses `scenes`/`composition` → `children` directly).
- If a `note` is ambiguous, make a reasonable minimal interpretation and say so in the summary.
- The studio hot-reloads on save, so the user sees the result immediately once you write the file.
