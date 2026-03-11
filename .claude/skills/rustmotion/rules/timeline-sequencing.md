# Rule: Timeline Sequencing

## Summary

Use the `timeline` field on a component's style to define sequential animation phases within a single scene. Each step triggers at a specific time with its own effects.

## Details

Timeline steps allow multi-phase animations without requiring separate scenes. Each step has an `at` time (in seconds) and an `animation` array. When the scene time reaches `step.at`, the step's animations activate with time resolved relative to `step.at`.

**Merging behavior:** Base `style.animation` effects play from the start. Timeline step effects are merged additively when they activate. Multiple steps can overlap.

## GOOD

```json
{
  "type": "card",
  "style": {
    "animation": [{ "name": "fade_in_up", "duration": 0.6 }],
    "timeline": [
      {
        "at": 2.0,
        "animation": [{ "name": "shake", "duration": 0.5 }]
      },
      {
        "at": 4.0,
        "animation": [{ "name": "fade_out", "duration": 0.8 }]
      }
    ]
  }
}
```

This creates: fade in (0-0.6s) → shake (2.0-2.5s) → fade out (4.0-4.8s).

## BAD

```json
{
  "style": {
    "timeline": [
      {
        "at": 0.0,
        "animation": [{ "name": "fade_in", "duration": 0.5 }]
      }
    ]
  }
}
```
Don't use timeline for initial entrance — use `style.animation` directly. Timeline is for subsequent phases.

## Tips

- Timeline steps support all animation effects: presets, keyframes, wiggle, orbit, glow.
- Use for patterns like: entrance → emphasis → exit within one scene.
- Step `at` values are relative to the component's animation start time (which includes `start_at` and stagger delays).
- Keep step `at` values within the scene duration.
