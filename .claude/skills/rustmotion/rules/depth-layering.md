# Rule: Visual Depth — Layering Elements in 3D Space

A flat scene with no depth cues looks like a poster. Depth makes elements feel physically placed in space. Use these techniques together — the more combined, the stronger the illusion.

---

## The five depth signals

| Signal | Far (background) | Close (foreground) |
|---|---|---|
| **Size** | Smaller | Larger |
| **Blur** | `backdrop-filter: blur` or low opacity | Sharp, full opacity |
| **Opacity** | 0.3–0.6 | 1.0 |
| **Shadow length** | None or short | Long, diffuse |
| **Motion speed** | Slow wiggle / slow orbit | Fast wiggle / large amplitude |

Combine at least three signals per scene for a convincing depth stack.

---

## Layer architecture: 3 planes

Design each scene in three planes. Every element belongs to exactly one:

| Plane | z-index | Role | Typical components |
|---|---|---|---|
| **Background** | 0 | Ambient texture, gradients, decorative shapes | `animated-background`, `shape` circles/blobs, `particle` |
| **Mid-ground** | 1 | Main content, cards, charts | `card`, `chart`, `codeblock`, `text` body |
| **Foreground** | 2 | Emphasis elements, badges, callouts | `badge`, `icon` hero, `text` headline |

Use `"style": { "z-index": N }` to enforce render order when elements overlap.

---

## 3D tilt for perceived depth

Adding `rotate_x`/`rotate_y` tilts an element so it reads as facing a direction — immediately reads as "in a 3D space". Always pair with a `box-shadow` to auto-get adaptive 3D shadows (shadow shifts direction with tilt angle — free, no extra config).

```json
{
  "type": "card",
  "style": {
    "width": 700,
    "height": 420,
    "background": "#1E293B",
    "border-radius": 24,
    "box-shadow": [{ "color": "#00000060", "offset-x": 0, "offset-y": 32, "blur": 80 }],
    "animation": [{
      "name": "keyframes",
      "keyframes": [
        { "property": "rotate_x", "keyframes": [{ "time": 0, "value": 12 }, { "time": 2.5, "value": 4 }], "easing": "ease_out" },
        { "property": "rotate_y", "keyframes": [{ "time": 0, "value": -10 }, { "time": 2.5, "value": -3 }], "easing": "ease_out" },
        { "property": "perspective", "keyframes": [{ "time": 0, "value": 900 }, { "time": 2.5, "value": 900 }] }
      ]
    }]
  }
}
```

Safe rotation ranges: `rotate_x` ±5°–15°, `rotate_y` ±5°–15°. Above ±25° the element distorts and text becomes unreadable.

---

## Seuil minimum d'opacité des blobs de fond

Les blobs décoratifs sur fond sombre doivent avoir une opacité **≥ 45%** pour être visibles.
Sur `#0f172a`, les valeurs `#RRGGBBAA` sûres :

| Opacité hex | Valeur | Résultat |
|---|---|---|
| `35` | 21% | Invisible sur fond sombre |
| `60` | 38% | Limite — acceptable uniquement si le fond est ≥ `#1e293b` |
| `80` | 50% | **Recommandé** pour fond sombre |
| `A0` | 63% | Fort — utilisé sur fond très sombre (`#080c18`) |

**BAD:** `"fill": { "type": "radial", "colors": ["#6366F135", "#6366F100"] }` → 21% → invisible.
**GOOD:** `"fill": { "type": "radial", "colors": ["#6366F180", "#6366F100"] }` → 50% → halo visible.

De même, le delta de luminosité entre le fond de scène et la carte doit être **≥ 8%**. `#080c18` → `#111827` = 4% = indiscernable. Préférer `#0f172a` (fond) + `#1e293b` (carte) + `box-shadow` coloré.

---

## Blur as depth cue (glassmorphism / atmospheric)

Background elements feel farther away when slightly blurred. Use `backdrop-filter` on foreground cards to make the background feel like a distinct plane behind them.

```json
{
  "type": "card",
  "style": {
    "background": "#FFFFFF18",
    "backdrop-filter": [{ "fn": "blur", "radius": 20 }],
    "border": { "width": 1, "style": "solid", "color": "#FFFFFF30" },
    "border-radius": 24
  }
}
```

**Rule:** `backdrop-filter` on the foreground card → background elements feel pushed back. Never blur the foreground itself — it breaks readability. See [glassmorphism.md](glassmorphism.md) for the complete recipe.

---

## Scale + opacity gradient across siblings

When cards or icons are in a row, give back-row items a smaller scale and lower opacity to simulate perspective receding.

```json
[
  { "type": "card", "style": { "opacity": 0.4, "transform": "scale(0.82)" }, "..." : "back" },
  { "type": "card", "style": { "opacity": 0.7, "transform": "scale(0.91)" }, "..." : "mid" },
  { "type": "card", "style": { "opacity": 1.0, "transform": "scale(1.00)" }, "..." : "front" }
]
```

Use `scale` steps of ~0.08–0.12 between planes. More than 3 planes starts looking mechanical.

---

## Shadow hierarchy

The further an element is from the "ground", the longer and softer its shadow.

| Plane | Recommended `box-shadow` |
|---|---|
| Background (low) | `{ "color": "#00000030", "offset-y": 8, "blur": 16 }` |
| Mid-ground | `{ "color": "#00000050", "offset-y": 20, "blur": 40 }` |
| Foreground (floating) | `{ "color": "#00000060", "offset-y": 40, "blur": 80 }` |

---

## GOOD: Full 3-plane scene

```json
{
  "duration": 5.0,
  "children": [
    {
      "type": "shape",
      "shape": "circle",
      "fill": { "type": "solid", "color": "#6366F120" },
      "style": {
        "width": 600,
        "height": 600,
        "z-index": 0,
        "animation": [{ "name": "wiggle", "property": "scale", "amplitude": 0.04, "frequency": 0.3, "seed": 7 }]
      }
    },
    {
      "type": "card",
      "style": {
        "width": 720,
        "height": 400,
        "background": "#1E293B",
        "border-radius": 24,
        "box-shadow": [{ "color": "#00000050", "offset-y": 24, "blur": 48 }],
        "z-index": 1
      },
      "children": [...]
    },
    {
      "type": "badge",
      "style": {
        "z-index": 2,
        "box-shadow": [{ "color": "#6366F180", "offset-y": 8, "blur": 24 }],
        "animation": [{ "name": "float_3d", "loop": true }]
      }
    }
  ]
}
```

---

## BAD: Everything at the same depth

```json
{
  "children": [
    { "type": "shape", "fill": "#6366F1" },
    { "type": "card",  "style": { "background": "#1E293B" } },
    { "type": "badge", "style": {} }
  ]
}
```
No size difference, no blur, no shadow hierarchy, no z-index — completely flat. ✗
