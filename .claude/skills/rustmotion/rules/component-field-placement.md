# Rule: Component Field Placement — What Goes Where

`style` is a `CssStyle` struct with `deny_unknown_fields`. Any non-CSS property inside `style` causes the component to fail deserialization — it is silently dropped and not rendered. This is the #1 source of invisible components in studio.

---

## animation — always at root, never inside style

**BAD (component silently dropped):**
```json
{ "type": "text", "style": { "font-size": 72, "animation": [{ "name": "fade_in_up" }] } }
```

**GOOD:**
```json
{ "type": "text", "animation": [{ "name": "fade_in_up", "duration": 0.6 }], "style": { "font-size": 72 } }
```

This applies to ALL components: `text`, `card`, `badge`, `icon`, `shape`, `image`, `codeblock`, etc.

---

## fill and stroke — root-level on shape

`shape` has dedicated root-level fields `fill` and `stroke`. They are NOT CSS properties.

**BAD:**
```json
{ "type": "shape", "shape": "circle", "style": { "fill": { "type": "radial", "colors": ["#6366F180", "#6366F100"] } } }
```

**GOOD:**
```json
{ "type": "shape", "shape": "circle", "fill": { "type": "radial", "colors": ["#6366F180", "#6366F100"] }, "style": { "z-index": 0 } }
```

---

## box-shadow — array format with kebab-case field names

`box-shadow` in CssStyle is `Vec<BoxShadow>` — it requires a JSON array, not a single object. Field names use kebab-case (`offset-y`, not `offset_y`).

**BAD:**
```json
"box-shadow": { "color": "#00000060", "offset_y": 20, "blur": 40 }
```

**GOOD:**
```json
"box-shadow": [{ "color": "#00000060", "offset-y": 20, "blur": 40 }]
```

Multiple shadows: `[{ "offset-y": 4, "blur": 8, "color": "#00000040" }, { "offset-y": 20, "blur": 60, "color": "#00000060" }]`

BoxShadow fields (all kebab-case): `color`, `offset-x`, `offset-y`, `blur`, `spread`, `inset`.

---

## Fields at root vs inside style — reference table

| Field | Location | Example |
|---|---|---|
| `animation` | Root | `{ "animation": [...], "style": {...} }` |
| `fill` (shape) | Root | `{ "fill": { "type": "radial", ... } }` |
| `stroke` (shape) | Root | `{ "stroke": { "color": "#fff", "width": 2 } }` |
| `timeline` | Root | `{ "timeline": [{ "at": 1.0, ... }] }` |
| `stagger` | Root | `{ "stagger": 0.15 }` |
| `size` | Root | `{ "size": { "width": 860, "height": 500 } }` |
| `position`, `x`, `y` | Root | `{ "position": "absolute", "x": 100, "y": 200 }` |
| `background` | `style` | `{ "style": { "background": "#1e293b" } }` |
| `border-radius` | `style` | `{ "style": { "border-radius": 24 } }` |
| `box-shadow` | `style` | `{ "style": { "box-shadow": [{ ... }] } }` |
| `opacity` | `style` | `{ "style": { "opacity": 0.7 } }` |
| `z-index` | `style` | `{ "style": { "z-index": 2 } }` |
| `font-size`, `color`, `text-align` | `style` | standard CSS properties |
| `flex-direction`, `gap`, `padding` | `style` | standard CSS properties |
