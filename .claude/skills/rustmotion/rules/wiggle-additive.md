# Rule: Wiggle Is Additive

Wiggle offsets apply additively on top of keyframe animations and presets. Combine a preset entrance with wiggle for ongoing procedural motion. All effects go in `style.animation` array with `"name": "wiggle"`.

**GOOD** (fade in, then gently float):
```json
{
  "type": "text",
  "content": "Floating text",
  "style": {
    "font-size": 48, "color": "#FFFFFF",
    "animation": [
      { "name": "fade_in_up", "delay": 0.2, "duration": 0.8 },
      { "name": "wiggle", "property": "translate_y", "amplitude": 5, "frequency": 2, "seed": 42 }
    ]
  }
}
```

**GOOD** (phone vibration with pure sine):
```json
{
  "type": "icon",
  "icon": "lucide:phone-off",
  "style": {
    "size": 64, "color": "#FFFFFF",
    "animation": [
      { "name": "scale_in" },
      { "name": "wiggle", "property": "translate_x", "amplitude": 12, "frequency": 90, "mode": "sine" },
      { "name": "wiggle", "property": "rotation", "amplitude": 8, "frequency": 75, "mode": "sine" }
    ]
  }
}
```

## Wiggle Modes

| Mode | Behavior | Use case |
| --- | --- | --- |
| `"noise"` (default) | Layered simplex noise — organic, random motion | Floating text, subtle drift, natural movement |
| `"sine"` | Pure sine wave — regular, mechanical oscillation | Phone vibration, heartbeat, pulsing, shaking |

## Frequency Guidelines

`frequency` is in **cycles per second (Hz)**. A frequency of `0.8` means one full oscillation every 1.25 seconds.

| Frequency | Period | Effect |
| --- | --- | --- |
| `0.5–1.0` | 1–2s | Slow, gentle float |
| `1.5–3.0` | 0.3–0.7s | Moderate wobble |
| `5–15` | 0.07–0.2s | Fast tremor/shiver |
| `60–120` | ~0.01s | Rapid vibration (phone buzzing) |

## Wiggle Properties

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `property` | string | required | Property to wiggle (`translate_x`, `translate_y`, `scale`, `rotation`, etc.) |
| `amplitude` | f64 | required | Maximum deviation (pixels for translate, degrees for rotation, factor for scale) |
| `frequency` | f64 | required | Cycles per second (Hz) |
| `mode` | string | `"noise"` | `"noise"` (layered simplex) or `"sine"` (pure sine wave) |
| `seed` | u64 | `0` | Random seed for reproducible results (noise mode only) |
| `octaves` | u32 | `3` | Noise complexity (noise mode only) |
| `phase` | f64 | `0.0` | Phase offset |
| `decay` | f64 | `null` | Exponential decay rate |
| `easing` | string | `null` | Remap noise through an easing curve |
