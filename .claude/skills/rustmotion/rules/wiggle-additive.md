# Rule: Wiggle Is Additive

Wiggle offsets apply additively on top of keyframe animations and presets. Combine a preset entrance with wiggle for ongoing procedural motion.

**GOOD** (fade in, then gently float):
```json
{
  "type": "text",
  "content": "Floating text",
  "position": { "x": 540, "y": 960 },
  "preset": "fade_in_up",
  "preset_config": { "delay": 0.2, "duration": 0.8 },
  "wiggle": [
    { "property": "translate_y", "amplitude": 5, "frequency": 2, "seed": 42 }
  ],
  "style": { "font-size": 48, "color": "#FFFFFF" }
}
```

## Wiggle Properties

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `property` | string | required | Property to wiggle (`translate_x`, `translate_y`, `scale`, `rotation`, etc.) |
| `amplitude` | f64 | required | Maximum deviation |
| `frequency` | f64 | required | Oscillations per second |
| `seed` | u64 | `0` | Random seed for reproducible results |
| `octaves` | u32 | `3` | Noise complexity |
| `phase` | f64 | `0.0` | Phase offset |
| `decay` | f64 | `null` | Exponential decay rate |
| `easing` | string | `null` | Remap noise through an easing curve |
