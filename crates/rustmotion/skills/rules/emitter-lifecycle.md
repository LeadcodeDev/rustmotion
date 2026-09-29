# Rule: Radial Particle Emitter (`emitter`)

For a warp-tunnel, a starfield, an ambient stream of light or embers — any
field of many small moving marks that should look alive and continuous, not
a fixed cohort that pops in together — use `emitter`. It supersedes the
deprecated `particle` and the hand-rolled `for-each` + `rand($seed, $i)` +
`sin($t)` recipe: neither has a real lifecycle, so both either freeze at one
age for the whole clip or require hundreds of individually keyframed nodes
to fake motion (a real study needed 460 nodes and hundreds of kilobytes of
JSON for three seconds of tunnel).

```json
{
  "type": "emitter",
  "origin": { "x": 960, "y": 540 },
  "rate": 180,
  "life": [0.7, 1.2],
  "direction": "radial",
  "speed": { "from": 200, "to": 1600, "easing": "ease_in" },
  "spawn_radius": [260, 470],
  "shape": "streak",
  "length": [60, 240],
  "color": "#EEF4FF",
  "width": 3,
  "seed": 7
}
```

| Field | Role |
|---|---|
| `origin` | `{x, y}` in the emitter's own box, in pixels. Defaults to the box centre. |
| `rate` | Average particles born per second. |
| `life` | `[min, max]` lifetime in seconds. Each particle draws its own value once, from `seed` and its index. |
| `direction` | Only `radial` today: born on a ring, travel straight outward. |
| `speed` | `{from, to, easing}` — pixels/second at birth and at death, and how the travel between them distributes over the particle's life. |
| `spawn_radius` | `[min, max]` ring, in pixels from `origin`, particles are born on. A non-zero minimum is what carves the dark "eye" out of the middle of a tunnel. |
| `shape` | `streak` (a short line aligned with the travel direction) or `dot`. |
| `length` | `[min, max]` streak length in pixels. Unused for `dot`. |
| `color` | Hex string. |
| `width` | Stroke width (`streak`) or diameter (`dot`) in pixels. |
| `seed` | Deterministic seed. Same seed, same instant, same pixels — always. |

## There is no particle count to set

`rate` and `life` are enough. How many particles are alive at any instant
follows from them: concurrency = `rate * average(life)`. A `rate: 180` with
`life: [0.7, 1.2]` (average 0.95 s) keeps roughly 171 particles alive at
once, continuously — there is no separate count field to keep in sync by
hand, and nothing to desync if you tune one without the other.

## The lifecycle is closed-form, not simulated

Every particle's age is derived directly from `(seed, index, time)`:

```
phase[i]    = a per-particle random offset into its own life, drawn once from seed and i
age(t)      = (t + phase[i]) mod life[i]
progress    = age / life[i]                      // 0 at birth, →1 at death
```

Nothing is stepped frame-to-frame and no history is kept. That's what makes
`still --time 1.7` on frame 51 of a 30fps render produce the exact same
pixels as decoding frame 51 out of a full `render` — both call the same pure
function of `time`. It also means the field never looks synchronized: two
particles never share a phase unless their random draws collide, so the
tunnel reads as a continuous flow of mixed ages from the very first frame,
not a cohort that was all born at `t=0`.

`speed.from`/`speed.to` describe the average velocity across a particle's
whole life; `speed.easing` then decides how that total travel distributes
across `progress` — `ease_in` spends most of the distance near the end,
which reads as acceleration outward. Each particle briefly fades in at
birth and fades out just before death, so appearance/disappearance is never
a hard pop.

## It's a decorative, full-bleed component

Like `particle`, `emitter` defaults to `100%` width/height and is treated as
**decorative**: it paints as a fullscreen layer behind the rest of the
scene's flex flow, and it is exempt from the viewport-overflow check (a
tunnel is expected to bleed past the frame edge by design). Give it an
explicit `style.width`/`style.height` if you want a contained effect inside
a card instead of the whole frame.

## `particle` is not extended for this

`particle`'s five presets (`confetti`, `snow`, `stars`, `bubbles`, `halo`)
are still there, deprecated, for compatibility — the fixed compositions
each preset draws have no life/death/respawn semantics and adding one to
that struct would just be `emitter` again with extra indirection. New
full-bleed particle work should reach for `emitter` directly.
