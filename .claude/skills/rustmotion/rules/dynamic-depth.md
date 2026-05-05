# Rule: Dynamic Depth — Multi-Element Parallax

Static depth (see [depth-layering.md](depth-layering.md)) places elements at different perceived distances. Dynamic depth *animates* each plane independently so the scene breathes and the spatial composition is felt over time, not just read visually.

Three independent mechanisms combine freely:

| Mechanism | Scope | Best for |
|---|---|---|
| **A. Per-element wiggle seeds** | One element | Uncorrelated floating per card/icon |
| **B. `float_3d` preset** | One element | Hero card with gentle 3D tilt loop |
| **C. Camera keyframes** | Whole scene | Cinematic zoom-in / slow pan |

---

## Mechanism A — Wiggle seeds for per-element parallax

Each element gets a different `seed`. Because seeds produce different noise curves, elements move asynchronously even at the same frequency — that desynchronization is the parallax effect.

**Rules:**
- Background elements: `frequency` 0.3–0.6 (slow drift), `amplitude` 3–6px
- Mid-ground elements: `frequency` 0.6–1.0 (medium), `amplitude` 4–8px
- Foreground elements: `frequency` 1.0–1.5 (faster), `amplitude` 6–12px
- Never reuse the same seed on two elements in the same scene
- Wiggle is always additive — it doesn't conflict with entrance presets

```json
[
  {
    "type": "shape",
    "animation": [
      { "name": "fade_in", "duration": 0.6 },
      { "name": "wiggle", "property": "translate_y", "amplitude": 5, "frequency": 0.4, "seed": 7 },
      { "name": "wiggle", "property": "translate_x", "amplitude": 3, "frequency": 0.3, "seed": 91 }
    ],
    "style": {}
  },
  {
    "type": "card",
    "animation": [
      { "name": "fade_in_up", "duration": 0.6 },
      { "name": "wiggle", "property": "translate_y", "amplitude": 6, "frequency": 0.7, "seed": 42 }
    ],
    "style": {}
  },
  {
    "type": "badge",
    "animation": [
      { "name": "scale_in", "delay": 0.3, "duration": 0.5 },
      { "name": "wiggle", "property": "translate_y", "amplitude": 8, "frequency": 1.2, "seed": 13 },
      { "name": "wiggle", "property": "scale", "amplitude": 0.04, "frequency": 0.8, "seed": 55 }
    ],
    "style": {}
  }
]
```

---

## Mechanism B — `float_3d` preset for hero depth

`float_3d` is a continuous preset that combines a `translate_y` float with a gentle `rotate_x`/`rotate_y` wobble. It reads as "the object is hovering in 3D space". Use it on the scene's primary element (hero card, main icon, featured image).

**Always pair `float_3d` with `"loop": true`.**

```json
{
  "type": "card",
  "size": { "width": 800, "height": 480 },
  "animation": [
    { "name": "scale_in", "duration": 0.7, "easing": "ease_out" },
    { "name": "float_3d", "loop": true }
  ],
  "style": {
    "background": "#1E293B",
    "border-radius": 28,
    "box-shadow": [{ "color": "#00000060", "offset-y": 40, "blur": 80 }]
  }
}
```

For mobile (larger scene), increase the `box-shadow` blur to 120 so the shadow movement on `float_3d` is visible.

**Do NOT apply `float_3d` to text components.** Rotating text mid-scene breaks readability.

---

## Mechanism C — Camera keyframes for cinematic parallax

The virtual camera transforms the entire scene uniformly. Animated `zoom` creates a "push in" that makes stationary elements feel they're being approached — the simplest form of cinematic parallax.

```json
{
  "duration": 5.0,
  "camera": {
    "keyframes": [
      { "property": "zoom", "values": [{ "time": 0, "value": 1.0 }, { "time": 4, "value": 1.08 }], "easing": "ease_in_out" }
    ]
  },
  "children": [...]
}
```

**Zoom range:** `1.0 → 1.05` (subtle) to `1.0 → 1.15` (dramatic). Beyond 1.2 elements exit the frame.

**Pan + zoom:**
```json
"keyframes": [
  { "property": "zoom", "values": [{ "time": 0, "value": 1.0 }, { "time": 4, "value": 1.1 }], "easing": "ease_in_out" },
  { "property": "x", "values": [{ "time": 0, "value": 0 }, { "time": 4, "value": -60 }], "easing": "ease_in_out" }
]
```

Camera applies **after** individual element animations — wiggle + float_3d still play on top.

---

## Combining all three: reference recipe

This is the "dynamism level 2" recipe for a hero scene on portrait 9:16 (1080×1920).

```json
{
  "duration": 6.0,
  "camera": {
    "keyframes": [
      { "property": "zoom", "values": [{ "time": 0, "value": 1.0 }, { "time": 5, "value": 1.06 }], "easing": "ease_in_out" }
    ]
  },
  "children": [
    {
      "type": "shape",
      "shape": "circle",
      "size": { "width": 800, "height": 800 },
      "style": {
        "fill": "#6366F118",
        "z-index": 0,
        "animation": [
          { "name": "fade_in", "duration": 1.2 },
          { "name": "wiggle", "property": "scale", "amplitude": 0.06, "frequency": 0.25, "seed": 7 },
          { "name": "wiggle", "property": "translate_y", "amplitude": 12, "frequency": 0.2, "seed": 91 }
        ]
      }
    },
    {
      "type": "card",
      "size": { "width": 900, "height": 540 },
      "style": {
        "background": "#1E293B",
        "border-radius": 32,
        "box-shadow": [{ "color": "#00000070", "offset-y": 48, "blur": 100 }],
        "z-index": 1,
        "animation": [
          { "name": "fade_in_up", "duration": 0.7 },
          { "name": "float_3d", "loop": true }
        ]
      },
      "children": [...]
    },
    {
      "type": "badge",
      "style": {
        "z-index": 2,
        "box-shadow": [{ "color": "#6366F180", "offset-y": 12, "blur": 32 }],
        "animation": [
          { "name": "scale_in", "delay": 0.4, "duration": 0.5 },
          { "name": "wiggle", "property": "translate_y", "amplitude": 10, "frequency": 1.1, "seed": 42 },
          { "name": "wiggle", "property": "scale", "amplitude": 0.05, "frequency": 0.9, "seed": 17 }
        ]
      }
    }
  ]
}
```

---

## Orbit for decorative background elements

`orbit` creates a circular/elliptical path with pseudo-3D scale modulation. Use it on non-content decorative elements (blobs, particles, icon decorations). Multiple elements with different `phase` values feel like a constellation.

```json
[
  { "animation": [{ "name": "orbit", "radius_x": 40, "radius_y": 24, "speed": 0.3, "depth": 0.12, "tilt": 15, "phase": 0.0 }] },
  { "animation": [{ "name": "orbit", "radius_x": 40, "radius_y": 24, "speed": 0.3, "depth": 0.12, "tilt": 15, "phase": 0.33 }] },
  { "animation": [{ "name": "orbit", "radius_x": 40, "radius_y": 24, "speed": 0.3, "depth": 0.12, "tilt": 15, "phase": 0.67 }] }
]
```

Phase steps: evenly distributed (`0`, `1/N`, `2/N`, ...). Same speed + different phase = elements stay locked in formation.

---

## Frequency hierarchy cheatsheet

| Element role | Wiggle frequency | Wiggle amplitude | Seeds |
|---|---|---|---|
| Background blob/shape | 0.2–0.4 Hz | 8–15px (translate) | Any, just unique |
| Mid-ground card | 0.5–0.8 Hz | 5–10px | Different from bg |
| Foreground badge/icon | 0.9–1.4 Hz | 6–12px | Different from mid |
| Text (body) | **Never wiggle primary text** | — | — |

---

## BAD: Same seed on every element

```json
{ "animation": [{ "name": "wiggle", "property": "translate_y", "amplitude": 6, "frequency": 0.8, "seed": 0 }] },
{ "animation": [{ "name": "wiggle", "property": "translate_y", "amplitude": 6, "frequency": 0.8, "seed": 0 }] }
```
Identical seed + frequency → elements move in perfect sync → no parallax, looks like a group animation bug. ✗

## BAD: `float_3d` on text

```json
{ "type": "text", "content": "Boost your sales", "style": { "animation": [{ "name": "float_3d", "loop": true }] } }
```
Rotating text mid-scene is disorienting and breaks readability. Use `float_3d` only on cards, icons, images, shapes. ✗
