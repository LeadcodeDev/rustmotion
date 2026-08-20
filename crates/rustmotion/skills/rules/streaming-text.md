# Rule: Text that "streams" (tokens arriving)

To depict a model's response being written, **don't use `typewriter`**: a typewriter reveals character by character at a fixed cadence, which reads as a typewriter, not as a stream of tokens. A model emits whole words, in uneven bursts, and each word settles visually instead of snapping in all at once.

```json
{
  "type": "text",
  "content": "Words arrive in uneven bursts, like tokens.",
  "style": {
    "font-size": 40,
    "color": "#E2E8F0",
    "animation": [{
      "name": "char_blur_in",
      "granularity": "word",
      "duration": 0.28,
      "stagger": 0.09,
      "jitter": 0.7,
      "seed": 12,
      "ink_from": "#475569",
      "blur": 6
    }]
  }
}
```

Three fields do all the work:

- **`granularity: "word"`** — the unit is the word, not the letter.
- **`jitter`** — offsets each unit's start by ±`jitter × stagger`. This is what breaks the metronomic cadence. 0.5–0.8 reads as streaming; past 1.0 words overlap and the reading order gets muddled.
- **`ink_from`** — each word starts in this colour and converges to `style.color` over its duration. A desaturated grey reproduces the "not yet accepted by the eye" token.

## `jitter` is deterministic, not random

The offsets are derived from `seed` and the unit's index, never from an RNG. This is a constraint, not a detail: frames are rendered out of order, in parallel, and sometimes in separate processes (`--frames a-b`). A word whose start depended on a random draw would jump between two neighbouring frames.

Changing `seed` reshuffles the rhythm without changing its statistics — useful so two neighbouring paragraphs don't "breathe" identically.

No unit can start before the effect's `delay`: a negative offset on the first unit would make it appear half-animated right from frame 0.

## Budget

`stagger × word count + duration` is the total settling time. For a 12-word sentence with `stagger: 0.09`, that's ~1.4s — check the scene is long enough; `validate` flags it if not.
