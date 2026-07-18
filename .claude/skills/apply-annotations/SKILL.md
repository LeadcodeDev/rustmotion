---
name: apply-annotations
description: Apply pending studio annotations from a Rustmotion scenario — for each open annotation, make the requested change at its target element, validate, and mark it resolved.
---

# Apply Annotations

Use this when a Rustmotion scenario has annotations (created in the studio's "Leave a comment for the agent" box) to apply.

## Where annotations live

- **JSON scenarios** (`foo.json`): in the scenario file's top-level `annotations` array.
- **HTML scenarios** (`foo.html` / `foo.htm`): in a sidecar file next to the source — `foo.annotations.json` — holding `{"annotations": [...]}` with exactly the same annotation object format. The HTML file itself never contains annotations. Always check for the sidecar when the source is HTML.

## Process

1. Read the `annotations` array — from the scenario JSON, or from `<stem>.annotations.json` for HTML sources. For each entry with `status: "open"`:
   - `target.pointer` is an RFC 6901 JSON Pointer to the element (e.g. `/scenes/2/children/5`).
   - `note` is the change request.
   - `frame` / `view` / `scene` give the moment in the video; `target.kind` is the component type (`text`, `card`, …).
2. For each open annotation, **apply the requested change** by editing the element at `target.pointer` — usually a property under its `style` object — interpreting `note`. Make the smallest edit that satisfies the note. For HTML sources, the pointer addresses the **transpiled** scenario structure; apply the change in the HTML source (inline `style` attribute of the corresponding element).
3. After each edit, run `rustmotion validate -f <file>` (schema + geometry). Both passes must succeed. If geometry fails (e.g. `unwrappable_text_overflow`), adjust (keep `wrap: true`, lower a `font-size`, …) and re-validate.
4. Set the annotation's `status` to `"resolved"` **in the same place you found it** — the scenario JSON, or the `<stem>.annotations.json` sidecar for HTML sources (do not delete it — the studio panel and `validate --fix` can strip resolved ones later).
5. Report a short summary: which annotations were applied and what changed.

## Rules

- Apply exactly what each `note` asks — no unrequested changes.
- Never break the `validate` passes; the scenario must stay renderable.
- Preserve the rest of the JSON (key order, other elements, the `annotations` array).
- Resolve the pointer against the **raw** scenario file (it addresses `scenes`/`composition` → `children` directly).
- If a `note` is ambiguous, make a reasonable minimal interpretation and say so in the summary.
- The studio hot-reloads on save, so the user sees the result immediately once you write the file.
