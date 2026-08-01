# Rule: Prefer Presets Over Manual Keyframes

Presets are simpler, less error-prone, and produce consistent motion design. Only use `keyframes` animation effects for custom behavior not covered by the 40 built-in presets (+ 6 char-only presets on `text`).

**GOOD:**
```json
{ "style": { "animation": [{ "name": "fade_in_up", "delay": 0.3, "duration": 0.8 }] } }
```

**BAD** (over-engineering a simple fade-in):
```json
{
  "style": {
    "animation": [{
      "name": "keyframes",
      "keyframes": [
        { "property": "opacity", "keyframes": [{ "time": 0.3, "value": 0.0 }, { "time": 1.1, "value": 1.0 }], "easing": "ease_out" },
        { "property": "translate_y", "keyframes": [{ "time": 0.3, "value": 30.0 }, { "time": 1.1, "value": 0.0 }], "easing": "ease_out" }
      ]
    }]
  }
}
```

## 45 Available Presets

| Category   | Presets |
| ---------- | ------ |
| Entrances  | `fade_in`, `fade_in_up`, `fade_in_down`, `fade_in_left`, `fade_in_right`, `slide_in_left`, `slide_in_right`, `slide_in_up`, `slide_in_down`, `scale_in`, `bounce_in`, `blur_in`, `rotate_in`, `elastic_in` |
| Exits      | `fade_out`, `fade_out_up`, `fade_out_down`, `slide_out_left`, `slide_out_right`, `slide_out_up`, `slide_out_down`, `scale_out`, `bounce_out`, `blur_out`, `rotate_out` |
| Continuous | `pulse`, `float`, `shake`, `spin` (use `"loop": true`), `float_3d` (floating + 3D rotation) |
| 3D         | `flip_in_x`, `flip_in_y`, `flip_out_x`, `flip_out_y`, `tilt_in` |
| Stroke     | `draw_in` (animate stroke drawing), `stroke_reveal` (draw_in + fade-in) |
| Special    | `typewriter`, `wipe_left`, `wipe_right` |
| Char (text only) | `char_scale_in`, `char_fade_in`, `char_wave`, `char_bounce`, `char_rotate_in`, `char_slide_up` |

`scale_in` and `scale_out` support `overshoot` (default 0.08 = 8%). Char presets support `stagger`, `granularity`, `easing`, and `overshoot`.

**Tout preset standard accepte `spring`** — un objet `{ "damping", "stiffness", "mass" }` qui remplace la courbe de mouvement du preset (translate/scale/rotate) par une physique de ressort ; l'opacity garde son ease. Exemple : `{ "name": "slide_in_left", "spring": { "damping": 10 } }`. Détails dans easing-guidelines.md. (Char presets et `tilt_in` : hors scope v1.)
