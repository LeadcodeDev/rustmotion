# Rule: Animation Completion Budget

Every animation must complete within its scene's duration. An animation that can't finish plays in a broken, truncated state — the component freezes mid-transition. The validator will catch this (schema error), but **always verify the budget before generating JSON**.

## Core formula

For a single animation on a component:

```
component.start_at + animation.delay + animation.duration ≤ scene_duration
```

For a stagger sequence of **N** elements where the first starts at `delay_first` with interval `stagger`:

```
delay_first + (N-1) × stagger + animation.duration ≤ scene_duration
```

## Worked examples

**3-element stagger, 0.2s interval, 0.6s duration:**
- Last element starts at: `0 + 2 × 0.2 = 0.4s`
- Finishes at: `0.4 + 0.6 = 1.0s`
- Minimum scene duration: **1.0s** + 0.5s dwell = **1.5s**

**5-element stagger, 0.2s interval, 0.6s duration:**
- Last element finishes at: `0 + 4 × 0.2 + 0.6 = 1.4s`
- Minimum scene duration: **1.4s** + 0.5s dwell = **1.9s**

**Component with start_at delay:**
```json
{ "start_at": 1.0, "animation": [{ "delay": 0.3, "duration": 0.6 }], "style": {} }
```
Total budget used: `1.0 + 0.3 + 0.6 = 1.9s` → scene must be at least **2.4s** (+ dwell).

## Exemptions

These are **not** subject to the budget rule:

- **Exit presets** (`fade_out`, `slide_out_left`, `slide_out_right`, `slide_out_up`, `slide_out_down`, `zoom_out`, `blur_out`) — intentionally overlap the scene end to provide a smooth transition out.
- **Continuous animations** with `"loop": true` — they run indefinitely, there is no completion event.
- **`marquee`** — continuous scroll by design.

## Character animations

Char/word animations have an implicit internal stagger. Total duration:

```
char_anim.delay + (char_count × char_anim.stagger) + char_anim.duration
```

Since `char_count` is unknown at planning time, use a conservative estimate: if `text_content` is N characters, multiply by `stagger` and add base duration. For short titles (5-15 chars), add 0.5–1.0s to the standard budget.

## BAD: Stagger exceeds scene duration

```json
{
  "duration": 2.0,
  "children": [
    { "type": "text", "content": "First",  "animation": [{ "name": "fade_in_up", "delay": 0.0, "duration": 0.6 }], "style": {} },
    { "type": "text", "content": "Second", "animation": [{ "name": "fade_in_up", "delay": 0.5, "duration": 0.6 }], "style": {} },
    { "type": "text", "content": "Third",  "animation": [{ "name": "fade_in_up", "delay": 1.0, "duration": 0.6 }], "style": {} },
    { "type": "text", "content": "Fourth", "animation": [{ "name": "fade_in_up", "delay": 1.5, "duration": 0.6 }], "style": {} }
  ]
}
```
Last animation finishes at `1.5 + 0.6 = 2.1s > 2.0s` (scene duration). Fourth element never completes.

## GOOD: Budget verified before committing duration

```json
{
  "duration": 3.5,
  "children": [
    { "type": "text", "content": "First",  "animation": [{ "name": "fade_in_up", "delay": 0.0, "duration": 0.6 }], "style": {} },
    { "type": "text", "content": "Second", "animation": [{ "name": "fade_in_up", "delay": 0.5, "duration": 0.6 }], "style": {} },
    { "type": "text", "content": "Third",  "animation": [{ "name": "fade_in_up", "delay": 1.0, "duration": 0.6 }], "style": {} },
    { "type": "text", "content": "Fourth", "animation": [{ "name": "fade_in_up", "delay": 1.5, "duration": 0.6 }], "style": {} }
  ]
}
```
Last animation finishes at `1.5 + 0.6 = 2.1s`. Scene is 3.5s → 1.4s dwell after. ✓

## Checklist before writing a scene's JSON

1. Sum: `last_start_at + last_delay + last_duration` → that's your animation budget.
2. Add `0.5s` dwell for simple scenes, `1.0s` for data-heavy or text-heavy scenes.
3. Set `scene.duration` to the maximum of that sum and the reading-time formula (see [scene-pacing.md](scene-pacing.md)).
4. If the budget overruns, either increase `duration` or reduce the number of stagger steps.
