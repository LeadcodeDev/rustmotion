# Rule: Sequential Component Reveals

## Timing semantics: `start_at`/`end_at` vs `style.animation` vs `timeline`

All three are processed by the rendering pipeline. Pick by intent:

| Field | Effect | Use for |
|---|---|---|
| `start_at` / `end_at` | Hard visibility window `[start_at, end_at)` — the component (and its subtree) paints nothing outside the window but **keeps its layout space** (CSS `visibility` semantics: siblings don't jump) | Hard cuts: appear/disappear without animation |
| `style.animation` | Animated effects; `delay` is absolute scene time | Entrances, exits, continuous effects |
| `timeline` | `[{ "at": t, "animation": [...] }]` — each step's animations run with `delay += at` | Grouping several timed animation phases on one component |

`start_at`/`end_at` control **visibility**, not animation timing. To delay an animation, use the animation's `delay` (or a `timeline` step's `at`).

```json
// Hard cut: card visible only between t=2 and t=5
{ "type": "card", "start_at": 2.0, "end_at": 5.0, "style": {}, "children": [...] }

// Timeline: pulse at t=1, fade out at t=3
{
  "type": "icon", "icon": "bell",
  "timeline": [
    { "at": 1.0, "animation": [{ "name": "pulse", "duration": 0.6 }] },
    { "at": 3.0, "animation": [{ "name": "fade_out", "duration": 0.5 }] }
  ]
}
```

**Combining:** a `start_at` hard cut composes with an entrance animation whose `delay` equals `start_at` (the window reveals the element exactly when the animation begins).

---

## Sequential reveals with animated handoff: the parallax pattern

To make components appear sequentially (each replacing the previous with an entrance + parallax exit), use:
1. All components at the same `position: absolute` coordinates
2. `style.animation` with **both** entrance and exit effects, using absolute scene-time `delay` values
3. Ascending `z-index` so later components stack on top during transitions

### Why it works

Animation preset keyframes use absolute scene time. Before the entrance `delay`, the preset's first keyframe returns `opacity: 0.0`, making the component invisible.

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
}
```

**Timing design:** Card N's `slide_out_up.delay` = Card N+1's `scale_in.delay`. They animate simultaneously for a smooth parallax handoff.

**Z-index rule:** Later cards get higher z-index so they appear above the exiting card during the overlap window.

**Layout note:** exited components still occupy layout space. For flowed (non-absolute) layouts where the element must disappear *and* free its slot, absolute positioning remains the pattern — `end_at` hides pixels, not layout.

---

## Sequential stagger (same-direction, no exit)

For items entering one by one without exits, use increasing `delay` on sibling elements — no absolute positioning needed:

```json
{ "type": "text", "content": "First",  "style": { "animation": [{ "name": "fade_in_up", "delay": 0.0, "duration": 0.6 }] } },
{ "type": "text", "content": "Second", "style": { "animation": [{ "name": "fade_in_up", "delay": 0.2, "duration": 0.6 }] } },
{ "type": "text", "content": "Third",  "style": { "animation": [{ "name": "fade_in_up", "delay": 0.4, "duration": 0.6 }] } }
```
