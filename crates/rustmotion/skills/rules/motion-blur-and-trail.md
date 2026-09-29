# Rule: `motion_blur` and `trail` — a ghost is never a flex item

`motion_blur` and `trail` (`style.animation`) paint ghost copies of the component at earlier instants (`BoxKind::Ghost` in `box_builder.rs`), at decreasing opacity. Up to the regression in issue #359 these ghosts misbehaved in three distinct ways. All three are fixed; this file documents the current behaviour and what is deliberately out of scope.

```json
{
  "type": "div",
  "style": {
    "width": 200, "height": 60, "background": "#FF4FB0",
    "animation": [{ "name": "motion_blur", "samples": 8, "shutter": 1.0 }]
  }
}
```

## 1. A ghost never takes up room in the flex

Before the fix, the ghost of an **in-flow** child (no `position: absolute`) became a full flex item in its own right — `samples` full-size copies on top of the real element, pushing the following siblings out of frame. A ghost is now always `position: absolute`, whether the node it duplicates is itself in flow or already positioned:

- Node already `position: absolute` → the ghost takes its exact `left`/`top` (unchanged behaviour).
- Node in flow → the ghost has no explicit inset, so Taffy would place it by the container's `justify-content`/`align-items`, like any absolute child with no `top`/`left` — often the wrong spot in an asymmetric layout. The render corrects that afterwards: `apply_ghost_layout_fixup` (`crates/rustmotion/src/engine/render/scene.rs`) shifts every ghost, and its whole subtree by the same delta, onto the position taffy actually resolved for its principal in the `LayoutResult` it has just produced. The ghost therefore lands exactly on the box of the node it duplicates, including under `space-between` or between siblings of different sizes — this is no longer an approximation.

## 2. A container's ghost carries its own subtree

A ghost is no longer built with `children: Vec::new()`. A `div` with a background and a `text` child now sees both duplicated — the subtree is rebuilt at the ghost's own instant (through `container_children`), not simply copied from the principal: a child with its own animation (delay, keyframes) is replayed at the ghost's instant, not at the scene's current one. That is what lets a whole card or mockup trail as one unit.

The ghost of a **measured** component (`text`, `counter`, `badge`, `table`, `rich_text`, `kbd`, `caption`, `number_wheel`) also carries its own `intrinsic` — without it the box measured to zero and nothing painted, exactly the "the text has no trail, the shape next to it does" symptom in the issue.

> There is no `scope: "self" | "subtree"` field to ghost only a container's box without its children — a container's ghost always carries its whole subtree. No verified use case needs it; add it if a real scenario asks.

## 3. `pointer.path` is sampled per ghost

A `pointer` moved by `path` no longer computes its position internally from `ctx.time`: `box_builder.rs` resolves `waypoint_offset` while building each node (ghost or principal) and injects it as `style.transform: translate(dx, dy)`, at that node's own instant — the very channel keyframed `translate_x`/`translate_y` already use. `Pointer::paint_content` no longer does that computation itself.

Each ghost gets its own local clock (`time_params`, the same table that drives `stagger_offset`), offset by exactly this sample's time delta. A pointer driven by `path` therefore trails along its trajectory, like one driven by keyframes.

> Structural trap found while fixing this: the opacity layer the engine opens for any node at `opacity < 1` (so every ghost) bounds its `SaveLayerRec` on the node's *untransformed* layout box. A displacement applied inside `paint_content` (as the old `canvas.translate` did) is invisible to that bounds computation and gets clipped away. `style.transform` moves the canvas *before* those bounds are computed, so it lines up correctly. Any future in-`Painter` positioning must go through `style.transform`, never through an ad-hoc `canvas.translate`.

## `mode: "smear"` — no ghosts at all

```json
{ "name": "motion_blur", "mode": "smear", "shutter": 1.0 }
```

Instead of stacking copies (`mode: "stack"`, the default), `smear` measures the component's displacement over the `shutter / fps` window preceding the current instant and puts a `{ "fn": "directional-blur", "angle": …, "radius": … }` filter straight on the principal — see [rules/directional-blur.md](directional-blur.md). Zero ghost nodes created: `samples` is ignored in `smear` mode. This is the recommended answer for fast, straight movement (a word crossing the frame) — a real directional blur instead of a staircase of copies visible at speed.

`samples: "auto"` (densifying `mode: "stack"`'s copies until they sit less than 2px apart, mentioned in issue #360 as a safety net if a real blur kernel turned out to be unreachable) is not implemented — `mode: "smear"` covers that need directly, and `samples` stays an integer in `1..=16`.
