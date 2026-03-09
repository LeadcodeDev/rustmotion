# Rule: Prefer Presets Over Manual Keyframes

Presets are simpler, less error-prone, and produce consistent motion design. Only use manual `animations` keyframes for custom behavior not covered by the 31 built-in presets.

**GOOD:**
```json
{ "preset": "fade_in_up", "preset_config": { "delay": 0.3, "duration": 0.8 } }
```

**BAD** (over-engineering a simple fade-in):
```json
{
  "animations": [
    { "property": "opacity", "keyframes": [{ "time": 0.3, "value": 0.0 }, { "time": 1.1, "value": 1.0 }], "easing": "ease_out" },
    { "property": "translate_y", "keyframes": [{ "time": 0.3, "value": 30.0 }, { "time": 1.1, "value": 0.0 }], "easing": "ease_out" }
  ]
}
```

Note: explicit `animations` override preset animations on the same property.

## 31 Available Presets

| Category   | Presets |
| ---------- | ------ |
| Entrances  | `fade_in`, `fade_in_up`, `fade_in_down`, `fade_in_left`, `fade_in_right`, `slide_in_left`, `slide_in_right`, `slide_in_up`, `slide_in_down`, `scale_in`, `bounce_in`, `blur_in`, `rotate_in`, `elastic_in` |
| Exits      | `fade_out`, `fade_out_up`, `fade_out_down`, `slide_out_left`, `slide_out_right`, `slide_out_up`, `slide_out_down`, `scale_out`, `bounce_out`, `blur_out`, `rotate_out` |
| Continuous | `pulse`, `float`, `shake`, `spin` (use `"loop": true`) |
| Special    | `typewriter`, `wipe_left`, `wipe_right` |
