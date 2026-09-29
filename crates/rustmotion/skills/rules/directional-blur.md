# Rule: Directional blur — `radius-x`/`radius-y`, `directional-blur`, `blur_x`/`blur_y`

`filter: [{ "fn": "blur", "radius": N }]` is isotropic: at a high value, a word crossing the frame bleeds as much vertically as horizontally and loses its shape. Three pieces cover an element stretched along its own axis of travel — a flat shape sliding, a word whipping past, a label dropping.

## Static per-axis blur: `radius-x` / `radius-y`

```json
{ "filter": [{ "fn": "blur", "radius-x": 40, "radius-y": 0 }] }
```

`Blur` has three fields, all optional: `radius` (isotropic, the historical behaviour), `radius-x`, `radius-y`. When `radius-x`/`radius-y` are given they win over `radius` **on their own axis**; the omitted axis falls back to `radius` if present, and is 0 otherwise. `{ "radius-x": 40, "radius-y": 0 }` gives a strictly horizontal blur; `{ "radius": 24 }` stays a plain isotropic blur — no existing scenario changes its render.

> Casing trap: both fields are kebab-case (`radius-x`), like everything else in `FilterFn` — consistent with `offset-x`/`offset-y` on `box-shadow` and `drop-shadow`. The snake_case spelling (`radius_x`, `radius_y`) is accepted as a parse alias — both spellings deserialise to the same field, but only `radius-x`/`radius-y` is re-emitted. `blur_x`/`blur_y` (below) are snake_case in their own right, because they live in the animatable-property namespace (`translate_x`, `scale.x`, …) rather than the filter-field one — two different conventions, each consistent with its immediate neighbours.

## Static diagonal blur: `directional-blur`

```json
{ "filter": [{ "fn": "directional-blur", "angle": 90, "radius": 40 }] }
```

For travel that is neither horizontal nor vertical. `angle` is in degrees (`0` = along +x, `90` = along +y), `radius` is the blur's length along that axis. Rendered by rotating the sampled content about the box centre, blurring on one axis, then rotating back — not a true oriented convolution, but visually equivalent for a reasonable blur, and not costly enough to justify a dedicated kernel.

## Animated blur: `blur_x` / `blur_y`

```json
{ "property": "blur_x", "keyframes": [{ "time": 0, "value": 40 }, { "time": 0.3, "value": 0 }] }
```

Two animatable properties (`KNOWN_MOTION_PROPERTIES`), alongside `blur` (which stays isotropic and unchanged). They land as a `Blur { radius-x, radius-y }` filter on the node — they combine with a static `radius`/`radius-x`/`radius-y` declared elsewhere by being appended to the `filter` list, not by replacing it.

## `motion_blur mode: "smear"` uses this automatically

See [rules/motion-blur-and-trail.md](motion-blur-and-trail.md) — `motion_blur`'s `smear` mode computes the component's instantaneous velocity and lays down a `{ "fn": "directional-blur", "angle", "radius" }` oriented along the actual displacement (`angle = atan2(dy, dx)`, `radius = hypot(dy, dx)`), with no need for the author to write it by hand. Diagonal travel therefore produces a streak at 45°, not the isotropic blob a `Blur { radius-x, radius-y }` pinned to the two screen axes would have given.

## What is not wired: per-character `blur_axis`

Issue #360 also proposed a **per-unit** blur axis on the `char_*` presets:

```json
{ "name": "char_blur_in", "granularity": "word", "blur_axis": "motion", "stretch": 1.3 }
```

Not implemented: per-character rendering (`char_*`) lives in the `text` component and its renderer, not in `animator.rs`/`box_builder.rs`. Adding `blur_axis`/`stretch` to the schema without the renderer consuming them would produce a field that is accepted but inert — exactly what this project avoids elsewhere (`text-autofit` on a component that does not implement it). Treat it as a separate piece of work, in `text.rs`/`renderer/text.rs`.
