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

Each point in `values` may carry its own `easing`, which governs the segment **starting** at that point and overrides the track's own `easing` for that segment alone — the same convention a component `Keyframe` already uses:

```json
{ "property": "x", "easing": "linear", "values": [
  { "time": 0, "value": 0, "easing": "ease_in" },
  { "time": 1, "value": 900 },
  { "time": 2, "value": 1200 }
] }
```

The first segment eases in, the second falls back to the track's `linear`. With no per-point easing anywhere, the track-level one applies to every segment, exactly as before.

> `"easing": "spring"` on a camera keyframe is accepted and resolves as **linear**. A component `Animation` drives a real spring from its own `spring` config; `CameraKeyframe` has no such field, so there is nothing to drive one. This is not new — it was already true of the track-level easing.

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

## The clip is pinned to the frame, the camera is not

The viewport clip does not travel with the camera. An element placed anywhere — including far outside `0..width` / `0..height` — is legitimate content a pan can bring into view, the way a real camera moves through a set larger than what it frames at any moment:

```json
{
  "version": "1.0",
  "video": { "width": 1920, "height": 1080, "fps": 30, "background": "#101018" },
  "scenes": [{
    "duration": 2,
    "camera": { "keyframes": [{ "property": "x", "easing": "linear",
      "values": [{ "time": 0, "value": 0 }, { "time": 2, "value": 2100 }] }] },
    "children": [{
      "type": "div", "position": "absolute", "x": 3000, "y": 440,
      "style": { "width": 200, "height": 200, "background": "#FF3366" }
    }]
  }]
}
```

At `t = 2` the element is centred on screen. Until #428 it was not: `clip_rect` was issued *after* the camera transform, and Skia bakes a clip into device space using the matrix in force when it is called — so the clip travelled with the camera and, past a pan of roughly the frame's own width, left the surface entirely and rendered a blank frame.

`validate` agrees with the renderer on this, and does not report such an element as a viewport overflow as long as the camera reaches it. See [rules/geometry-safety.md](geometry-safety.md).

## See also

Camera motion blur (#362) is now implemented — `camera.motion_blur`, see
[rules/camera-motion-blur.md](camera-motion-blur.md).
