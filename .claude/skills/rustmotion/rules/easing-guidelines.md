# Rule: Use Appropriate Easing for Motion Design

| Use case | Recommended easing |
|---|---|
| UI element entrances | `ease_out` or `ease_out_cubic` |
| Element exits | `ease_in` or `ease_in_cubic` |
| Continuous/looping | `linear` |
| Playful bouncy motion | `spring` with `{ "damping": 15, "stiffness": 100, "mass": 1 }` |
| Counter number animation | `ease_out` |
| Camera-like zoom | `ease_in_out` |
| Smooth subtle reveals | `spring` with `{ "damping": 200 }` |

Entrance presets already use appropriate easing internally.

## Available Easing Functions

`linear`, `ease_in`, `ease_out`, `ease_in_out`, `ease_in_quad`, `ease_out_quad`, `ease_in_cubic`, `ease_out_cubic`, `ease_in_expo`, `ease_out_expo`, `spring`

### Spring Physics

When easing is `spring`, configure with:
```json
{
  "easing": "spring",
  "spring": { "damping": 15, "stiffness": 100, "mass": 1 }
}
```
