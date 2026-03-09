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

**GOOD** (phone vibration with pure sine):
```json
{
  "type": "icon",
  "icon": "lucide:phone-off",
  "position": { "x": 540, "y": 960 },
  "preset": "scale_in",
  "wiggle": [
    { "property": "translate_x", "amplitude": 12, "frequency": 90, "mode": "sine" },
    { "property": "rotation", "amplitude": 8, "frequency": 75, "mode": "sine" }
  ],
  "style": { "size": 64, "color": "#FFFFFF" }
}
```

## Wiggle Modes

| Mode | Behavior | Use case |
| --- | --- | --- |
| `"noise"` (default) | Layered simplex noise — organic, random motion | Floating text, subtle drift, natural movement |
| `"sine"` | Pure sine wave — regular, mechanical oscillation | Phone vibration, heartbeat, pulsing, shaking |

## Wiggle Properties

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `property` | string | required | Property to wiggle (`translate_x`, `translate_y`, `scale`, `rotation`, etc.) |
| `amplitude` | f64 | required | Maximum deviation |
| `frequency` | f64 | required | Radians per second (e.g. 90 ≈ rapid vibration) |
| `mode` | string | `"noise"` | `"noise"` (layered simplex) or `"sine"` (pure sine wave) |
| `seed` | u64 | `0` | Random seed for reproducible results (noise mode only) |
| `octaves` | u32 | `3` | Noise complexity (noise mode only) |
| `phase` | f64 | `0.0` | Phase offset |
| `decay` | f64 | `null` | Exponential decay rate |
| `easing` | string | `null` | Remap noise through an easing curve |
