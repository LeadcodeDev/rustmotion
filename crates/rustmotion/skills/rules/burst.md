# Rule: `burst` — the ring of strokes an element throws when it pops

`burst` (`style.animation`) paints a ring of short strokes that shoot outward from just off the node's own box edge, then retract. It is the accent on a pill, a badge or a checkmark at the moment it *pops* — the graphic equivalent of the little "tchak".

```json
{
  "type": "badge",
  "text": "Shipped",
  "style": {
    "animation": [
      { "name": "pop_in", "delay": 0.2, "duration": 0.45 },
      { "name": "burst", "delay": 0.28, "duration": 0.4, "count": 10,
        "length": 34, "gap": 10, "width": 4, "color": "#FFB020",
        "seed": 3, "jitter": 0.25 }
    ]
  }
}
```

| Field | Role | Default |
|---|---|---|
| `delay` | Wait before the strokes shoot out (s) | `0` |
| `duration` | Full out-and-back duration (s); the head reaches the far end of its track at the halfway point | `0.4` |
| `count` | Number of strokes in the ring (clamped internally to `1..=64`) | `8` |
| `length` | Length of each stroke's track, in px, measured outward from `gap` | `40` |
| `gap` | Distance in px between the node's box edge and the near end of every stroke | `12` |
| `width` | Stroke width in px | `4` |
| `color` | Stroke colour (hex string) | `"#FFB020"` |
| `seed` | Seed for the per-stroke angle, length and phase jitter | `0` |
| `jitter` | How far a stroke may stray from its even share of the ring, as a fraction of the spacing between two strokes; it also scales the per-stroke length and phase | `0.2` |

## The head leaves, the tail catches up

Each stroke is defined by two ends that cross the same track once each: the **head** runs out over the first half of the window (`ease_out`), the **tail** follows over the second (`ease_in`). The stroke lengthens, reaches full length at the halfway point, then closes **outward** — it flies away and vanishes, it does not retract into the box.

One consequence matters: at both ends of the window the head and the tail are in the same place, so the stroke has zero length. **The zero-at-both-ends guarantee here is geometric, not only temporal.** `burst_progress` does short-circuit outside `[delay, delay + duration)`, but even if a frame landed *inside* the window within one ULP of its boundary — which happens: `delay + duration - delay != duration` in floating point as soon as `duration` is not exactly representable, `0.4` for instance — the measured stroke would have zero length and nothing would be painted. Both protections exist, and they do not cover the same case.

## Nothing that belongs to the node is touched

The strokes are painted **over** the node, **outside its box**, after its own render and inside its transform. Three consequences:

- They take **no layout space** — a `burst` never pushes a flex neighbour.
- They follow the node: if it rotates or moves (`pop_in`, `transform`), the ring rotates and moves with it.
- `gap` guarantees nothing bites into the box. `gap: 0` puts the strokes against the edge; a negative value is clamped to `0`, never an overlap.

The ring can still leave the **viewport** if the node sits near an edge. The geometry validator does not see it (it inspects layout boxes, and a `burst` has none): it is up to the layout to leave `gap + length` of margin around the node.

## The animation budget applies

Like [shatter.md](shatter.md), `burst` counts toward [animation-completion-budget.md](animation-completion-budget.md): `start_at + delay + duration ≤ scene_duration`. It is not an exempt exit preset.

In practice it fires **slightly after** the entry it accents, not at the same time: the splash answers the pop, it does not precede it. In the example above, `pop_in` starts at `0.2` and `burst` at `0.28`.

## Degenerate cases

- `count: 0` is treated as `1` — a single stroke, which reads more like an accident than a burst.
- `length: 0` or `width: 0` paints nothing at all: no error, the effect is simply inert.
- `jitter: 0` gives a perfectly regular ring, which reads like a technical-diagram sunburst; `jitter: 1` lets a stroke encroach on its neighbour's slot, which reads like a spatter. In between, `0.2` to `0.35` is the range that looks hand-drawn.
- A zero or negative `duration` disables the effect on every frame, like `chromatic_aberration` and `shatter`.

## It is not `emitter`

`burst` is **bounded**: one round trip, `count` strokes, then nothing. `emitter` ([emitter-lifecycle.md](emitter-lifecycle.md)) is a **continuous stream**: particles are born, travel, die and are reborn for as long as the scene lasts. A badge appearing → `burst`. A warp tunnel or a starfield → `emitter`.
