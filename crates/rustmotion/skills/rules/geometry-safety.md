# Rule: Keep Content Inside the Viewport

No textual content may bleed out of the device viewport. The renderer enforces this through three opt-in mechanisms, all checked by `rustmotion validate`.

## 1. Text wrapping (`style.white-space`)

`text` is constraint-aware: it wraps to its parent's allocated width by default. There is no `style.wrap` field — that boolean belonged to the pre-CSS `LayerStyle` model and no longer exists in `CssStyle` (`deny_unknown_fields` rejects it and silently drops the component). Wrapping is controlled by the standard CSS `white-space` property instead:

- Default: unset / `"normal"` (and `"pre-line"`, `"pre-wrap"`, `"break-spaces"`) — wraps at parent width or `max-width`, whichever is smaller.
- `white-space: "nowrap"` or `"pre"` → the text renders on one line. The validator measures its natural (unbounded) width and fails with `unwrappable_text_overflow` if that width exceeds the box. Only set this when you intentionally want a single line and a finite `max-width` + reasonable `font-size` guarantee it fits (e.g. a title, a ticker-like label — not a marquee, which has its own component).

```json
{ "type": "text", "content": "Long sentence...", "style": { "white-space": "nowrap", "max-width": 800 } }
```

## 2. Text shrink-to-fit (`style.text-autofit`)

`text` and `gradient_text` accept `text-autofit: true`, which reduces their font size until the content fits the resolved box — width, and height when taffy resolves one.

```json
{ "type": "text", "content": "A headline too long for its box",
  "style": { "width": "320px", "height": "90px", "font-size": 120, "text-autofit": true } }
```

Use it when the copy is data-driven and you cannot know in advance whether it fits — a label coming from a `for-each`, a headline injected through a variable. Do **not** reach for it to paper over a layout you can simply size correctly: shrinking is a fallback, not a design.

Three things to know:

- **It has a floor.** Shrinking stops at a calibrated legibility threshold and never goes below it. If the text still does not fit at the floor, the geometry violation is **still reported** — `text-autofit` narrows that failure class, it does not silence it.
- **`white-space` still decides whether the text wraps**; `text-autofit` only decides at what size. They compose.
- **Only these two components implement it.** Declaring it on a `caption` or a `table` is inert — those painters never read it.

On a canvas taller than 1080, `validate` warns that autofit may shrink below the legibility floor for that frame height. That warning is about the *rendered* size, not the declared one.

## 3. Container `style.overflow`

CSS-like semantics: `visible` (default) lets children bleed; `hidden` clips at the parent box. The validator only fails when content escapes the **viewport**, not a `visible` parent — a badge sticking out of a card is legal.

```json
{ "type": "card", "style": { "overflow": "hidden" } }
```

## What the validator catches

`rustmotion validate -f scenario.json` reports four geometry violation kinds:

- `viewport_overflow` — absolute bbox crosses the device edge
- `unwrappable_text_overflow` — `white-space: "nowrap"`/`"pre"` but natural width > available width
- `content_overflows_box` — wrapping text needs more room than the box it was actually assigned, typically a paragraph inside a card with a fixed `height` too small for it. Text painters never clip themselves, so this paints outside its box even when the box sits comfortably inside the frame — which is why the viewport check alone never caught it.
- `animated_text_overflow` — an animated transform (scale/translate/wiggle/orbit) pushes the bbox out of the viewport at some sampled time. Only checked with `--strict-anim` (default runs check the resting, untransformed layout only).

`marquee` and `cursor` are exempt (their job is to bleed). A node is also exempt when it clips itself, or when any ancestor clips it — `overflow` set to anything other than `visible`. Deliberate bleed under a clipping parent is a composition technique, not a defect: that is how you get giant type running off the frame.

## CLI usage

```bash
rustmotion validate -f scenario.json                       # human-readable
rustmotion validate -f scenario.json --report report.json  # JSON report
rustmotion validate -f scenario.json --fix                 # safe auto-fixes
rustmotion validate -f scenario.json --strict-anim         # per-frame check, adds animated_text_overflow
rustmotion validate scenario.json --lenient             # warnings only
```

`--fix` rewrites the file in place:
- `unwrappable_text_overflow` → removes `style.white-space`, so the text falls back to the `normal` default and wraps again. Non-destructive: it only ever deletes the property that caused the violation. If you want the line to stay unbroken, widen the box or lower `font-size` by hand instead of running `--fix`.

Position/size clamping (`viewport_overflow`) is never auto-applied — fix those by hand too.

## A camera makes "outside the viewport" a question about time

An element placed outside `0..width` / `0..height` is not automatically an overflow when the scene has a `camera`. The validator resolves the camera's own `keyframes` — not just its static `x`/`y`/`zoom` — and samples them across the scene, stopping at `scene.freeze_at`. The element is reported only if **no** sampled instant of that motion brings it fully into frame.

```json
{
  "version": "1.0",
  "video": { "width": 1920, "height": 1080, "fps": 30, "background": "#101018" },
  "scenes": [{
    "duration": 2,
    "camera": { "keyframes": [{ "property": "x", "easing": "linear",
      "values": [{ "time": 0, "value": 0 }, { "time": 2, "value": 2100 }] }] },
    "children": [{
      "type": "div", "position": "absolute", "x": 3000, "y": 440,
      "style": { "width": 200, "height": 200, "background": "#FF3366" }
    }]
  }]
}
```

This passes: the element is never inside `0..1920` on its own, but at `t = 2` the camera has panned to `x = 2100` and the element is centred. A camera that only ever reaches `x = 500`, with the element at `x = 9000`, is still reported — a camera on the scene is not a blanket exemption.

Two limits worth knowing. The check asks whether the element is in frame at *some* instant, not at every instant: an element that leaves the frame mid-pan is not reported by the default pass, and `--strict-anim` is what samples per frame. And the camera fold accounts for `x`, `y` and `zoom` only — `rotation` and an animated `origin` are not folded in, so an element brought into view purely by a camera rotation is still reported.

## When to use what

| Symptom | Fix |
|---|---|
| Long sentence cut at viewport edge | leave `white-space` unset (default wraps) and ensure parent has finite width |
| Need a single-line title that must fit | set `max-width` and a small enough `font-size`, leave `white-space` unset |
| Marquee / ticker text that intentionally scrolls past edges | use `marquee` (exempt) — never `text` with `white-space: "nowrap"` |
| Badge protruding from a card on purpose | container has `overflow: visible` (default) — no change needed |
| Hard-clip children to a card border | container `style.overflow: "hidden"` |
| Animated element (wiggle/orbit/keyframe scale) might drift off-screen | run `rustmotion validate --strict-anim` to sample frames, not just the resting layout |
