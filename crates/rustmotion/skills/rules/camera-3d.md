# Tilting the whole plane: `camera.rotate_x` / `rotate_y` / `perspective`

`style.depth` parallax knows about translation and zoom. To tilt a whole plane,
the workaround used to be copying the same `rotate_x` keyframes onto every group
— and **each group ended up with its own vanishing point**, which does not read
as a camera but as cards each spinning in its own corner.

```json
"camera": {
  "perspective": 1400,
  "keyframes": [
    { "property": "rotate_y",
      "values": [{ "time": 0, "value": -16 }, { "time": 2, "value": 16 }],
      "easing": "ease_in_out" }
  ]
}
```

| Field | Default | Role |
|---|---|---|
| `rotate_x` | `0` | tilt about the horizontal axis, in degrees |
| `rotate_y` | `0` | tilt about the vertical axis |
| `perspective` | `0` | viewing distance in pixels; `0` = orthographic projection |

All three animate through `keyframes`, like `zoom` and `rotation`.

## One vanishing point, and depth-scaled rotation

Perspective is applied **once**, around the camera's origin. That is the whole
difference from the version copied onto each group.

The rotation, in turn, is scaled by each direct scene child's `style.depth` — the
same rule parallax and `camera.focus` already follow. A plane at `depth: 3` tilts
three times as much as one at `depth: 1`, and that is what gives the sense of
volume.

## Two traps

**It only applies to direct scene children.** Like parallax: a plane is a layer.
Putting `depth` on a node buried in a subtree does not tilt it on its own.

**`perspective: 0` is not "no rotation".** It is an orthographic tilt — the shape
skews without converging. For a camera that reads as a camera you need a
distance, and `1200`–`1800` covers most framings.

## See also

Camera motion blur (#362) is now implemented — `camera.motion_blur`, see
[rules/camera-motion-blur.md](camera-motion-blur.md).
