# Rule: Sequential Component Reveals

## Critical: `start_at`, `end_at`, and `timeline` are NOT processed in the rendering pipeline

These fields exist on component structs but are **silently ignored** for box-based components (card, div, text, icon, etc.) in the new CSS rendering pipeline. Using them produces no effect — the component is always visible regardless of `start_at`.

**Only `style.animation` effects are processed.**

---

## Sequential reveals: the correct pattern

To make components appear sequentially (each replacing the previous with an entrance + parallax exit), use:
1. All components at the same `position: absolute` coordinates
2. `style.animation` with **both** entrance and exit effects, using absolute scene-time `delay` values
3. Ascending `z-index` so later components stack on top during transitions

### Why it works

Animation preset keyframes use absolute scene time. Before the entrance `delay`, the preset's first keyframe returns `opacity: 0.0`, making the component invisible. No `start_at` needed.

For `scale_in` with `delay: 2.0, duration: 0.5`:
- t < 2.0 → opacity = 0 (first keyframe value at t=2.0 is 0.0)
- t = 2.0–2.15 → opacity 0→1, scale 0→1
- t ≥ 2.5 → opacity = 1, scale = 1

### Example: 3 cards appearing one after another with parallax exit

```json
{
  "type": "card",
  "position": "absolute",
  "x": 200, "y": 400,
  "style": {
    "width": 1500, "padding": 40, "border-radius": 20,
    "background": "#0C1A2E", "z-index": 1,
    "animation": [
      { "name": "scale_in",    "delay": 0.5, "duration": 0.5 },
      { "name": "slide_out_up","delay": 2.5, "duration": 0.55 }
    ]
  },
  "children": [...]
},
{
  "type": "card",
  "position": "absolute",
  "x": 200, "y": 400,
  "style": {
    "width": 1500, "padding": 40, "border-radius": 20,
    "background": "#0C1A2E", "z-index": 2,
    "animation": [
      { "name": "scale_in",    "delay": 2.5, "duration": 0.5 },
      { "name": "slide_out_up","delay": 4.5, "duration": 0.55 }
    ]
  },
  "children": [...]
},
{
  "type": "card",
  "position": "absolute",
  "x": 200, "y": 400,
  "style": {
    "width": 1500, "padding": 40, "border-radius": 20,
    "background": "#0C1A2E", "z-index": 3,
    "animation": [
      { "name": "scale_in", "delay": 4.5, "duration": 0.5 }
    ]
  },
  "children": [...]
}
```

**Timing design:** Card N's `slide_out_up.delay` = Card N+1's `scale_in.delay`. They animate simultaneously for a smooth parallax handoff.

**Z-index rule:** Later cards get higher z-index so they appear above the exiting card during the overlap window.

---

## Sequential stagger (same-direction, no exit)

For items entering one by one without exits, use increasing `delay` on sibling elements — no absolute positioning needed:

```json
{ "type": "text", "content": "First",  "style": { "animation": [{ "name": "fade_in_up", "delay": 0.0, "duration": 0.6 }] } },
{ "type": "text", "content": "Second", "style": { "animation": [{ "name": "fade_in_up", "delay": 0.2, "duration": 0.6 }] } },
{ "type": "text", "content": "Third",  "style": { "animation": [{ "name": "fade_in_up", "delay": 0.4, "duration": 0.6 }] } }
```

---

## What NOT to do

```json
// ❌ start_at is ignored — card is always visible
{
  "type": "card",
  "start_at": 2.0,
  "timeline": [{ "at": 0.0, "animation": [{"name": "scale_in"}] }],
  "style": { "z-index": 1 }
}

// ❌ end_at is ignored — card never disappears
{
  "type": "card",
  "end_at": 3.0,
  "style": {}
}
```
