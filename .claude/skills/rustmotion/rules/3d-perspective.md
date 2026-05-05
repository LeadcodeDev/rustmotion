# Rule: 3D Perspective Transforms

## Summary

Use `rotate_x`, `rotate_y`, and `perspective` as keyframe properties to create true 3D perspective transforms on any component.

## Details

The 3D engine uses a Skia M44 (4x4 matrix) for real perspective rendering — not fake CSS-style transforms. Any component with non-zero `rotate_x` or `rotate_y` renders through this 3D pipeline.

**3D Adaptive Shadow:** When a component has a `box-shadow` and 3D rotation, the shadow automatically shifts and scales based on the tilt angles. The shadow moves opposite to the rotation direction, simulating a ground-plane light source. No extra configuration needed — it's automatic.

## Animatable 3D Properties

| Property | Description | Typical range |
|---|---|---|
| `rotate_x` | Rotation around X axis (forward/backward tilt) | -30 to 30 degrees |
| `rotate_y` | Rotation around Y axis (left/right tilt) | -30 to 30 degrees |
| `perspective` | Perspective distance (lower = more dramatic) | 400–1200 pixels |

## GOOD

```json
{
  "style": {
    "box-shadow": [{ "color": "#00000060", "offset-x": 0, "offset-y": 20, "blur": 60 }],
    "animation": [{
      "name": "keyframes",
      "keyframes": [
        { "property": "rotate_x", "keyframes": [{ "time": 0, "value": 20 }, { "time": 2, "value": 8 }], "easing": "ease_out" },
        { "property": "rotate_y", "keyframes": [{ "time": 0, "value": -15 }, { "time": 2, "value": -5 }], "easing": "ease_out" },
        { "property": "perspective", "keyframes": [{ "time": 0, "value": 800 }, { "time": 2, "value": 800 }], "easing": "linear" }
      ]
    }]
  }
}
```

## BAD

```json
{
  "style": {
    "animation": [{
      "name": "keyframes",
      "keyframes": [
        { "property": "rotate_x", "keyframes": [{ "time": 0, "value": 60 }], "easing": "linear" }
      ]
    }]
  }
}
```
Extreme rotation angles (>30°) distort the component and make text unreadable. Always keep perspective defined when using 3D rotation.

## Tips

- Use `perspective: 800` as a safe default. Lower values (400) = more dramatic 3D. Higher values (1200) = subtler.
- Pair with `backdrop-blur` and semi-transparent backgrounds for glassmorphism cards.
- The `tilt_in` preset is a shortcut for a simple 3D entrance — use custom keyframes when you need animated tilt that changes over time.
- Add a `box-shadow` to get automatic 3D adaptive shadows for free.
