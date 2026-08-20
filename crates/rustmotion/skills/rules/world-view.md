# Rule: The `world` View — Real Continuity Between Beats

Every other composition mechanism in rustmotion — `scenes`, `slide` views, `transition` — cuts. A `slide`-view `transition` composites two already-rendered RGBA frame buffers (`crates/rustmotion-core/src/engine/transition.rs`): fade, wipe, zoom, flip, iris, slide, dissolve all blend or clip two flat images. No live element, no shared background, no camera state survives from scene A into scene B — by the time the transition runs, both scenes have already been fully painted in isolation.

The **`world` view** is different: one virtual camera moves continuously through a 2D space, and scenes are positioned in that space rather than replacing each other. This is the only mechanism that produces a video reading as one continuous shot across several beats — the effect used in things like the Machina/Aikido-style "endless dolly" promo.

Use a `world` view whenever the brief calls for "no perceptible scene boundaries" or "continuous camera movement." Use `slide` views (the default) for everything else — they're simpler and every other rule in this skill assumes them.

---

## 1. Turning it on: `composition`

`world` only exists inside `composition`, the typed-views array. Root-level `scenes` is sugar for a single implicit `slide` view and cannot mix with `composition` (using both is a validation error).

```json
{
  "version": "1.0",
  "video": { "width": 1920, "height": 1080, "fps": 30 },
  "composition": [
    {
      "type": "world",
      "camera_pan_duration": 1.0,
      "camera_easing": "ease_in_out",
      "background": { "preset": "halo", "zones": [ { "color": "#6366F1AA", "x": 0.3, "y": 0.4, "radius": 0.5 } ] },
      "scenes": [
        { "duration": 2.5, "children": [ { "type": "text", "content": "Beat one" } ] },
        { "duration": 2.5, "children": [ { "type": "text", "content": "Beat two" } ] }
      ]
    }
  ]
}
```

View-level fields relevant to `world`: `camera_pan_duration` (f64, default `0.8`), `camera_easing` (easing enum, default `"ease_in_out"`), `background` (shared, view-wide — see §4). A `slide` view can precede or follow a `world` view in the same `composition`; the outer `view.transition` field (a plain frame-buffer crossfade/wipe, same engine as scene transitions) handles the join between views.

---

## 2. The coordinate model — read this before writing `world-position`

This is the part that catches people twice, because nothing in the JSON shape hints at it.

### `world-position` is a camera waypoint, not the scene's origin

Source: `crates/rustmotion/src/engine/world.rs`, `WorldTimeline::build` (~line 74-86) and `crates/rustmotion/src/engine/render/scene.rs`, `render_world_frame_scaled` (~line 838-855). Both read a scene's `world-position` the same way: it's the point the camera centers on when it arrives at that scene, and it's also where the scene's own center gets translated to. If you omit it, the default is:

```
x = video_width / 2 + i * video_width
y = video_height / 2
```

(`i` = the scene's index in the view's `scenes` array.) That default already lays scenes out as a horizontal filmstrip, each one `video_width` px apart, so **most of the time you don't need to set `world-position` at all** — just add scenes and let the camera dolly sideways through them.

**The mistake:** because the default formula uses `i * video_width`, it's tempting to hand-set the second scene's `world-position.x` to `video_width` (`1920` on a 1920-wide video) — as if `world-position` were the scene's left edge, filmstrip-style. It isn't. `1920` centers the camera on the seam between scene 1 and scene 2, framing half of each. The value that actually centers the camera on scene 2 is `video_width * 1.5` = `2880`, matching what the default formula already produces for `i = 1`.

```json
// BAD — thinks world-position is a left-edge/origin coordinate.
// Validates cleanly (nothing checks this value), then frames the
// seam between beat 1 and beat 2 instead of beat 2's center.
{ "duration": 2.0, "world-position": { "x": 1920, "y": 540 } }

// GOOD — world-position is the camera's CENTER waypoint for this scene.
{ "duration": 2.0, "world-position": { "x": 2880, "y": 540 } }
```

Nothing in `rustmotion validate` catches the bad version — it's a semantically wrong but structurally valid waypoint, not a schema or geometry error. The only way to catch it is to know the formula.

Only set `world-position` explicitly when the path isn't a straight horizontal filmstrip: a vertical or diagonal move between beats, a zoom that revisits an earlier beat's location (see `persist` in §3), or non-uniform spacing.

### Children use scene-local coordinates, always

A scene's `children` are laid out exactly like a `slide`-view scene: `0..video_width` × `0..video_height`, independent of that scene's `world-position`. The camera translation is applied to the whole scene as one block, after layout — a child never needs to know where its scene sits in world space.

This was verified against the geometry validator (`crates/rustmotion/src/cli/commands/geometry.rs`, `validate_geometry`): it checks every scene's children against `(scenario.video.width, scenario.video.height)` regardless of that scene's `world-position`. Placing a child at what looks like the world-space X (e.g. `2880` for the second beat) is rejected as viewport overflow:

```json
// Scene 2 has "world-position": {"x": 2880, ...}. A child positioned at
// x: 2880 (thinking in world space) fails:
{ "type": "shape", "shape": "circle", "position": "absolute", "x": 2880, "y": 540,
  "style": { "width": 100, "height": 100 } }
```
```
ERROR: shape (viewport overflow)
  bbox: [2880, 540] -> [2980, 640]   (viewport: 1920x1080)
  hint: shift x to fit [0..1920], current right edge is 2980
```

The same child at `x: 200` (scene-local) validates. Both hypotheses were tested against the actual validator output above — world-space child coordinates are rejected, scene-local coordinates are correct.

### Casing gotcha

`world-position` (on `Scene`) is kebab-case (`#[serde(rename = "world-position")]` in `crates/rustmotion-core/src/schema/scenario.rs`). Its neighbour on the same `Scene` struct, `freeze_at`, is snake_case with no rename at all. This is a real inconsistency in the schema, not a typo — write each field with the casing shown here, don't infer one from the other.

---

## 3. The junction: `camera_pan_duration`, `camera_easing`, `persist`

- **`camera_pan_duration`** (default `0.8`s, view-level) — how long the camera takes to glide from one scene's waypoint to the next. The pan is centered on the scene boundary: it starts `camera_pan_duration / 2` before the boundary and ends the same amount after.
- **`camera_easing`** (default `"ease_in_out"`, view-level) — easing curve for the camera's position interpolation between waypoints.
- During the pan, both the outgoing and incoming scene are visible and cross-fade (opacity ramps 1→0 / 0→1 over the pan window) while the camera moves — this is what replaces the hard cut. See `WorldTimeline::visible_scenes_at` in `world.rs`.
- **`persist`** (bool, default `false`, per-scene) — keeps a scene's content rendering (fully opaque) after its own time window ends, instead of disappearing. Combine with an explicit `world-position` that a later beat revisits, to make earlier content still be there when the camera swings back — a genuine callback, not a re-render of the same scene.
- A per-scene `transition` field is **ignored inside a `world` view** — `build_world_view_tasks` in `crates/rustmotion/src/encode/video/tasks.rs` never reads it. Continuity is entirely driven by `camera_pan_duration`/`camera_easing`; don't add `transition` objects to scenes inside a `world` view expecting them to do anything.

**Don't confuse this with the `camera_pan` *transition type*.** `slide`-view scenes accept `"transition": { "type": "camera_pan" }`, which is a different, narrower mechanism: a two-scene jump that composites a static background from scene A with sliding foreground layers (`camera_pan_transition` in `transition.rs`), offset by the delta between the two scenes' `world-position` values. It requires both scenes to already share the same background (only scene A's background is rendered) and only ever handles one jump — it is not multi-beat continuity. Worse, at a **view boundary** (the `transition` field on a `composition` entry, used when entering/leaving a view), `camera_pan` silently falls back to a plain crossfade (`TransitionType::CameraPan => blend_fade` in `transition.rs`) — no panning at all. For an actual continuous multi-beat camera, use a `world` view, not a `camera_pan` transition.

---

## 4. The ambient layer: `view.background` with `halo`, never per-scene shapes

A `world`-view video usually wants a soft ambient glow behind everything, visible continuously through every beat and every pan. The instinct is to add a blurred `shape` circle to each scene — don't. Two things go wrong:

1. **A large blurred glow that bleeds past scene edges (the usual placement) fails viewport validation.** Only `marquee` and `cursor` are exempt from geometry checks today (`is_exempted` in `geometry.rs`) — a decorative shape gets the same `viewport overflow` error as any content element:
   ```
   ERROR: shape (viewport overflow)
     bbox: [-300, -300] -> [500, 500]   (viewport: 1920x1080)
     hint: reposition the component to stay inside the viewport
   ```
2. **Wrapping it in `overflow: hidden` passes validation — and breaks the seamless effect.** Each `world`-view scene is painted independently into its own `video_width × video_height` block, then translated into place (`render_world_frame_scaled` in `engine/render/scene.rs`). An `overflow: hidden` container sized to the scene turns that block into a hard-edged rectangle. During a camera pan the camera straddles two scenes' blocks at once, so that edge sits in plain view mid-frame — the exact "hard cut" a `world` view exists to avoid, just wearing a disguise.

The correct place for this layer is `view.background` with the `halo` preset — one glow layer shared by the whole view, drawn once behind every scene, continuous through every pan:

```json
{
  "type": "world",
  "background": {
    "preset": "halo",
    "zones": [
      { "color": "#6366F1AA", "x": 0.25, "y": 0.35, "radius": 0.55 },
      { "color": "#22D3EEAA", "x": 0.75, "y": 0.65, "radius": 0.45 }
    ]
  },
  "scenes": [ ... ]
}
```

`zones` fields: `color` (hex, may itself carry an alpha channel like `#RRGGBBAA`), `x`/`y` (fraction of width/height, default `0.5`), `radius` (fraction of `max(width, height)`, default `0.4`). This same `background` field also exists on individual scenes and at the scenario root — but for a `world` view's ambient layer, put it on the **view**, not a scene, so it doesn't disappear or restart at scene boundaries.

---

## 5. Full recipe: a validated 3-beat continuous video

Verified with `./target/release/rustmotion validate`. Demonstrates: `world-position` math for a horizontal dolly (§2), the ambient `halo` at view level (§4), and `persist` for a callback beat (§3) — beat 3 revisits beat 1's neighborhood, and beat 1's headline is still there because it persisted.

```json
{
  "version": "1.0",
  "video": { "width": 1920, "height": 1080, "fps": 30, "background": "#0a0a14" },
  "composition": [
    {
      "type": "world",
      "camera_pan_duration": 1.0,
      "camera_easing": "ease_in_out",
      "background": {
        "preset": "halo",
        "zones": [
          { "color": "#6366F1AA", "x": 0.25, "y": 0.35, "radius": 0.55 },
          { "color": "#22D3EEAA", "x": 0.75, "y": 0.65, "radius": 0.45 }
        ]
      },
      "scenes": [
        {
          "duration": 2.5,
          "persist": true,
          "world-position": { "x": 960, "y": 540 },
          "children": [
            { "type": "text", "content": "The problem", "style": { "font-size": 72, "color": "#FFFFFF", "font-weight": "bold", "text-align": "center", "animation": [{ "name": "fade_in_up", "duration": 0.6 }] } },
            { "type": "text", "content": "Scenes cut. Nothing survives the cut.", "style": { "font-size": 36, "color": "#94A3B8", "text-align": "center", "animation": [{ "name": "fade_in_up", "delay": 0.2, "duration": 0.6 }] } }
          ]
        },
        {
          "duration": 2.5,
          "world-position": { "x": 2880, "y": 540 },
          "children": [
            { "type": "text", "content": "The world view", "style": { "font-size": 72, "color": "#FFFFFF", "font-weight": "bold", "text-align": "center", "animation": [{ "name": "fade_in_up", "duration": 0.6 }] } },
            { "type": "text", "content": "One camera, gliding between beats.", "style": { "font-size": 36, "color": "#94A3B8", "text-align": "center", "animation": [{ "name": "fade_in_up", "delay": 0.2, "duration": 0.6 }] } }
          ]
        },
        {
          "duration": 2.5,
          "world-position": { "x": 960, "y": 800 },
          "children": [
            { "type": "text", "content": "Full circle", "style": { "font-size": 44, "color": "#FFFFFF", "font-weight": "bold", "text-align": "center", "animation": [{ "name": "fade_in_up", "duration": 0.6 }] } },
            { "type": "text", "content": "Beat one is still there, thanks to persist.", "style": { "font-size": 30, "color": "#94A3B8", "text-align": "center", "animation": [{ "name": "fade_in_up", "delay": 0.2, "duration": 0.6 }] } }
          ]
        }
      ]
    }
  ]
}
```

`rustmotion validate` on this file: `Valid scenario: 3 scene(s) in 1 view(s)`.

### Checklist

- [ ] Composition uses `"type": "world"`, not `"slide"`
- [ ] Ambient glow, if any, is `view.background` with `preset: "halo"` — not a per-scene `shape`
- [ ] `world-position` set only where the path deviates from the default horizontal filmstrip; when set, it's the scene's **center**, computed as `video_width * (beat_index + 0.5)` for a simple sideways dolly
- [ ] Children keep scene-local coordinates (`0..video_width`, `0..video_height`) regardless of the scene's `world-position`
- [ ] No per-scene `transition` inside the `world` view — junctions are controlled by `camera_pan_duration`/`camera_easing`
- [ ] `persist: true` only on scenes meant to still be there if the camera revisits their spot later
