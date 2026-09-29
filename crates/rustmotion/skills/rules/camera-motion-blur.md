# Rule: `camera.motion_blur` — blurring the shot, not the components

`motion_blur` in `style.animation` trails **one component** along its own animation (see [rules/motion-blur-and-trail.md](motion-blur-and-trail.md)). It sees nothing when the thing moving is the camera: on a fast pan the whole frame should smear, and every component is standing still in its own coordinates.

`camera.motion_blur` is the other half. It opens a shutter window ending at the frame's own time, renders `samples` sub-frame exposures inside it, and averages them — the same thing a real camera does with a real shutter.

```json
"camera": {
  "motion_blur": { "samples": 8, "shutter": 1.0 },
  "keyframes": [
    { "property": "x", "easing": "ease_in_out",
      "values": [{ "time": 0, "value": 0 }, { "time": 2, "value": -900 }] }
  ]
}
```

| Field | Type | Default | Role |
|---|---|---|---|
| `samples` | int | `8` | Sub-frame exposures averaged together, clamped to `1..=16`. Each one is a full render. |
| `shutter` | f64 | `0.5` | Exposure length as a multiple of one frame (`shutter / fps` seconds), ending at the frame's own time. `0.5` is the 180-degree shutter of a film camera; `1.0` leaves it open for the whole frame and smears twice as far. |

Absent (the default), nothing changes: the frame renders exactly as it did before.

## Cost is a direct multiplier, and you only pay it while the camera moves

`samples` full renders per frame is not an implementation detail to be optimised away later — it is what the feature *is*. Measured at 1920×1080 on a scene with a grid background, three shapes and two text blocks, taking the slope over 180 frames so process startup drops out:

| | per frame |
|---|---|
| no `camera.motion_blur` | **4.1 ms** |
| `samples: 8`, `shutter: 1.0`, camera panning and zooming | **8.8 ms** |

2.2× end to end, not 8×, because encoding and frame setup are paid once per frame whatever the render costs. The *render* portion does scale by `samples`, so the ratio climbs toward `samples` as the scene itself gets heavier. Budget for it before putting this on a long shot.

Every frame resolves the camera pose (`x`, `y`, `zoom`, `rotation`, plus `scene.shake`) at both ends of the shutter window. If they match, the frame takes the ordinary single-render path and costs nothing extra — a held shot in the middle of a scenario that blurs its pans is free.

## It is a shutter, so content blurs too

The sub-frames resample *everything* at their own instant, not just the camera transform. A component animating fast during a camera pan smears as well. That is what a physical exposure does, and pinning content to the nominal frame while only the camera moves would mean threading a second, independent time channel through the render core.

Practical consequence: do not combine a fast `camera.motion_blur` with a component that is itself mid-`char_*` stagger unless you want both softened.

## An animated background does not smear

`scene["animated-background"]` is painted in screen space and does not pan with the camera — verified: its grid columns sit at the same x at `time: 0` and at `time: 1.0` of a 900px pan. Since it does not move, averaging the sub-frames leaves it crisp. If you want the backdrop to carry the pan, it has to be real content in the scene tree, not a background preset.

## Not to be confused with

- **`style.animation: [{ "name": "motion_blur" }]`** — per component, ghost copies or a `smear` filter. Does nothing for a camera move.
- **the `whip` / `zoom_blur` transitions** — those composite two already-rendered frame buffers between scenes; they never re-render anything. See [rules/whip-transition.md](whip-transition.md).
- **`scene.shake`** — a declarative, beat-synced handheld or impact wobble, added to the camera pose on top of `camera.keyframes` (`camera_pose_at`). It works with no `camera` block at all. Reach for it for a shaky-cam feel; `camera.motion_blur` only blurs motion that is already happening, pan or shake, and creates none of its own.
- **`camera.focus`/`aperture`** — defocus by depth plane, a static property of the shot rather than a function of its movement. See [rules/depth-of-field.md](depth-of-field.md).

## Where it lives

In `render_frame_v2_scaled` (`crates/rustmotion/src/engine/render/scene.rs`), not in the encode loop: `render/mod.rs` re-exports scene's public items through an explicit named list, and the frame path already funnels through that one function. The encode side calls exactly what it called before and gets an already-averaged buffer back, so `apply_post_effects` still runs once on the final composited frame rather than once per sub-sample.
