# Rule: Time Remapping on Containers (time_scale / time_offset)

Containers (`flex`, `card`, `grid`, `container`/`div`, `positioned`) accept two
optional fields that remap time for their **entire subtree**:

```
t_local = (t_global - time_offset) * time_scale
```

- `time_scale` (default `1.0`) — playback speed of the subtree. `0.5` = children
  run at half speed (slow motion); `2.0` = double speed. **Must be > 0** —
  `rustmotion validate` rejects `0` and negatives.
- `time_offset` (default `0.0`) — seconds before the subtree's timeline starts.
  `2.0` = the subtree behaves as if the scene started 2 s later.

Everything inside follows the remap: animation presets, keyframes, timeline
steps, `start_at`/`end_at` windows, stagger, motion blur ghosts, and internal
animations (counter progress, `draw_in`, terminal typewriter…).

**Slow-motion example** — the card's children fade in at half speed:

```json
{
  "type": "card",
  "time_scale": 0.5,
  "children": [
    { "type": "text", "content": "Slow entrance",
      "style": { "animation": [{ "name": "fade_in", "duration": 1.0 }] } }
  ]
}
```

The fade lasts 1 s of *local* time = 2 s of real scene time. Budget the scene
duration accordingly: a child animation ending at `t_local` finishes at
`t_global = t_local / time_scale + time_offset`.

**Cascade** — nested remapped containers compose: a `time_scale: 0.5` container
inside another `time_scale: 0.5` container runs its subtree at 0.25× speed.
Offsets compose too (each `time_offset` is expressed in its parent's already
remapped time).

**start_at / end_at** — expressed in the subtree's local time. A child with
`start_at: 1` inside a `time_scale: 0.5` container appears at 2 s of real time.

**scene_duration stays global** — the physical scene window is not remapped.
Duration-relative behaviors (e.g. a counter spanning the whole scene) keep
their real-time span; only the elapsed-time input is remapped.

**HTML dialect** — use the snake_case attribute name:

```html
<rm-flex time_scale="0.5" time_offset="1">…</rm-flex>
```

The kebab-case form `time-scale` does NOT map to the field and is ignored.
