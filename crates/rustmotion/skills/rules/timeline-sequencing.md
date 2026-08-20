# Rule: Sequential Component Reveals

## Timing semantics: `start_at`/`end_at` vs `style.animation` vs `timeline`

All three are processed by the rendering pipeline. Pick by intent:

| Field | Effect | Use for |
|---|---|---|
| `start_at` / `end_at` | Hard visibility window `[start_at, end_at)` — the component (and its subtree) paints nothing outside the window but **keeps its layout space** (CSS `visibility` semantics: siblings don't jump) | Hard cuts: appear/disappear without animation |
| `style.animation` | Animated effects; `delay` is absolute scene time | Entrances, exits, continuous effects |
| `timeline` | `[{ "at": t, "animation": [...] }]` — each step's animations run with `delay += at` | Grouping several timed animation phases on one component |
| `timeline` + `style` states | `[{ "at": t, "style": {...} }]` — the style state applies from `at` onwards (box-model properties snap); with `style.transition` on the component, `opacity` (all components) and `color` (text/counter) interpolate smoothly | State changes: "turns red at 2s", "dims to 20% at 3s" |

```json
// Text turns blue at t=1 over 0.5s; background snaps at the same instant
{
  "type": "text", "content": "Status",
  "timeline": [{ "at": 1.0, "style": { "color": "#3b82f6", "background": "#0f172a" } }],
  "style": { "color": "#ef4444", "transition": { "duration": 0.5, "easing": "ease_in_out" } }
}
```

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

## Ce que `style.transition` lisse réellement

`style.transition` fait interpoler les changements posés par un `timeline` — mais **seulement pour certaines propriétés**. Toutes les autres sautent à l'instant du pas.

Interpolées aujourd'hui :

| Propriété | Restriction |
|---|---|
| `opacity` | — |
| `color` | sur `text` / `counter` |
| `background` | couleur solide uniquement |
| `border-radius` | rayon uniforme, en px absolus |

Tout le reste saute. Ce n'est pas silencieux : dès que `style.transition` est posé, `rustmotion validate` inspecte les diffs entre états de `timeline` et **nomme chaque propriété qui ne sera pas lissée**, avec la raison.

Trois raisons distinctes, et le message le dit :

- **Propriété de layout** (`width`, `height`, `margin`, `padding`, `gap`, `font-size`, `top`/`left`…) — interpoler demanderait de relancer le layout à chaque frame échantillonnée. Le message suggère l'alternative : `transform: translate` ou `scale`, qui sont côté peinture et s'interpolent, elles.
- **Propriété discrète** (`display`, `position`, `overflow`, `font-weight`, `text-align`…) — il n'existe aucune valeur intermédiaire. Le saut est le comportement CSS attendu, pas une limite de rustmotion, et le message le précise pour t'éviter de chercher un bug.
- **Peinture non supportée** (`transform`, `box-shadow`, `filter`, `clip-path`…) — continue en principe, pas encore implémenté.

Les unités relatives (`%`, `em`, `rem`, `vw`, `vh`) et les rayons par coin sont refusés à l'interpolation et signalés : avant le layout, ces unités n'ont pas de base fiable.

**En pratique :** pour animer une taille ou une position, préfère `transform` à `width`/`top`. C'est ce que le validateur te dira, et c'est aussi ce qui coûte le moins cher à rendre.
