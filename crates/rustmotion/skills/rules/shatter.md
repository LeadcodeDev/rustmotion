# Rule: `shatter` — Voronoi fragments that fly apart (or assemble)

`shatter` (`style.animation`) cuts a node's already-painted render — background, border, children, its whole subtree — into deterministic polygonal cells (a Voronoi partition) and sends each piece flying away from an origin point, with its own rotation and its own fade. It is the building block for a card, a thumbnail or a pane of glass that shatters and reveals what is behind it — see issue #378.

## The shape

```json
{
  "type": "div",
  "style": {
    "width": 400, "height": 300,
    "background": "#1B1F3B",
    "animation": [{
      "name": "shatter",
      "delay": 1.2,
      "duration": 0.7,
      "mode": "out",
      "pieces": 24,
      "seed": 7,
      "origin": { "x": 0.5, "y": 0.5 },
      "spread": 1.0,
      "spin": 90,
      "depth": 0.4,
      "fade": true
    }]
  }
}
```

| Field | Role | Default |
|---|---|---|
| `delay` | Wait before the shards start moving (s) | `0` |
| `duration` | How long the dispersal (or the assembly, in `mode: "in"`) takes (s) | `0.6` |
| `mode` | `"out"` / `"in"` / `"hold"` — see below | `"out"` |
| `pieces` | Number of Voronoi cells (clamped internally to `1..=64`) | `24` |
| `seed` | Seed for the partition and for every shard's jitter (direction, spin, depth) | `0` |
| `origin` | The point shards fly away from (or converge toward in `"in"`), as a fraction `0..1` of the node's own box — not pixels | `{ "x": 0.5, "y": 0.5 }` |
| `spread` | Radial travel multiplier at full dispersal, relative to the node's own diagonal | `1.0` |
| `spin` | Maximum rotation in degrees at full dispersal; each shard's sign and magnitude are drawn from `seed` | `90` |
| `depth` | Per-shard scale modulation at full dispersal — the same "0 = none" semantics as `OrbitConfig.depth` (some shards grow, others shrink) | `0.4` |
| `fade` | Drop each shard's opacity to zero at full dispersal (and the reverse in `mode: "in"`) | `true` |

## `mode` decides which end is the intact node

- **`"out"`** (default): assembled at `delay`, dispersed at `delay + duration`. **Outside that window the effect contributes strictly nothing** — the node is pixel for pixel identical to one with no `shatter` in its `animation` list. That is the same hard short-circuit `chromatic_aberration` and `zoom_blur` use (see [chromatic-aberration.md](chromatic-aberration.md)) rather than a fade that merely approaches zero.
- **`"in"`** is the mirror: dispersed at `delay`, assembled at `delay + duration` — and, like `"out"`, outside its window the node renders as if the effect were absent. The difference between the two modes is therefore **not** the state at the boundaries (both are "no effect" before `delay` and after `delay + duration`) but the direction `progress` runs through the window: `0` (assembled) → `1` (dispersed) in `"out"`, the reverse in `"in"`.
- **`"hold"`** plays the same dispersal as `"out"` but **freezes** at full dispersal once `delay + duration` is reached, instead of returning to the whole node — it never reconverges. It is the only one of the three whose final state differs from a node without the effect.

A trap not to reproduce elsewhere: do not confuse "outside the window" with `progress` near 0 or 1. `active_shatter` (`paint_pass.rs`) returns `None` — not `Some(0.0)` or `Some(1.0)` — outside the window; that is a different code branch (`paint_node_visual` directly, with no rasterisation and no clipping), not the same function evaluated at a boundary.

## The animation budget applies

`shatter` counts toward [animation-completion-budget.md](animation-completion-budget.md) like any entrance: `start_at + delay + duration ≤ scene_duration`. It is **not** an exempt exit preset — even in `mode: "out"` or `"hold"`, where the effect reads like an exit. A shatter meant to land on the cut must therefore fall exactly on the end of the scene rather than overrun it: `delay + duration == scene_duration`. Otherwise the validator raises the usual budget error.

## Trap: `origin` is a fraction, not pixels

Unlike `TransformOrigin` (CSS, `LengthPercentage`), `shatter.origin` is a pair of floats `0..1` relative to the node's box — `{ "x": 0.5, "y": 0.5 }` is the centre, `{ "x": 0.0, "y": 0.0 }` the top-left corner. Passing pixels is not a schema error (the field accepts any float) but an origin point outside the box, so every shard leaves in nearly the same direction instead of radiating.

## How it is painted

The node's subtree is painted once into a **dedicated** raster surface the size of its own box (`box_layout.width × height`, local coordinates — the same move `paint_inflated_material`/`silhouette_alpha_field` make to rasterise and read pixels back). That capture disables the hit map (`PaintContext.hits: None`): while the node is fragmented, its children are not coherent click targets — only the node itself stays clickable, at its original rectangle, exactly as if it were not breaking.

The Voronoi partition comes from seed points on an approximate grid (`√pieces` columns), each perturbed by a deterministic hash of `(seed, index)` — not a uniformly random point, which would produce degenerate slivers. Each cell is computed by successively clipping the bounding rectangle against the perpendicular bisector of every other point (Sutherland-Hodgman, `O(pieces²)` — negligible up to 64 pieces). Direction, travel magnitude, spin sign and magnitude, and per-shard depth are all hashes of distinct `(seed, index, salt)` triples — so two renders of the same file at the same instant are byte-identical.

For each shard the order of canvas operations matters: **translate/rotate/scale first, clip (`clip_path`) second**, in that exact order. Clip before transform and the mask stays at its original position while the image underneath slides: the shard never visually moves, only its content shifts inside a static hole. That bug was observed and fixed during implementation; a dedicated test (`shatter_paints_ink_outside_the_nodes_own_box_where_an_intact_node_does_not`) turns red on the revert.

## Degenerate cases

- `pieces: 0` or `1` is treated as `1` (clamped internally): the whole box is one shard, which simply translates, rotates and scales as a block — no error, just a "shatter" degenerated into a plain exit.
- `spread: 0` pins the shards in place: only spin, depth and fade remain visible, a "dislocation without flight" variant.
- `duration: 0` (or negative) disables the effect on every frame, like `chromatic_aberration`.
- `fade: false` leaves shards at full opacity even when fully dispersed — useful with `mode: "hold"` for a shattered composition that has to stay legible.
