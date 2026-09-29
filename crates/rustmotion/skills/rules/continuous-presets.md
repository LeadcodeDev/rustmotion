# Rule: Continuous Presets Need loop: true

The presets `pulse`, `float`, `shake`, and `spin` are continuous animations. Without `"loop": true`, they play once and stop.

**GOOD:**
```json
{ "style": { "animation": [{ "name": "float", "loop": true }] } }
```

**BAD:**
```json
{ "style": { "animation": [{ "name": "float" }] } }
```

Continuous presets: `pulse`, `float`, `shake`, `spin`.

## `speed` + `direction`: only four backgrounds accept being scrolled

`direction` translates the background's texture. That only makes sense for a pattern that is **periodic under translation** and has no motion of its own:

| Preset | `direction` | Why |
|---|---|---|
| `grid_dots`, `grid_lines`, `pixel_grid`, `heropattern` | **active** | Tiled patterns, drawn with a whole period of margin on each side. The outer scroll is their only motion. |
| `gradient_shift` | **inert** | `speed` already drives the gradient's rotation sense (`direction` means `cw`/`ccw` here), and the shader is painted over the frame rect with **no margin**: any translation left an uncovered band. |
| `concentric_circles` | **inert** | It already computes its own `offset = (time * speed) % spacing`. Translating a radial pattern moves its centre — that was a double animation. |
| `halo` | **inert** | It animates its zones itself. |

On those last three, declaring a `direction` now produces **nothing at all** (verified frame by frame, pixel for pixel). This is a visible rendering change for an existing scenario that declares one — but the motion being removed is the motion that dragged the background off the frame, not an effect worth keeping.

> `pixel_grid` has its own `motion` field (`twinkle`, `sweep`), which translates nothing: it composes with the scroll rather than doubling it. Its default, `motion: none`, makes it a still texture — exactly `grid_dots`'s case.
