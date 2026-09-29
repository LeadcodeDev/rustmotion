# Rule: two animations on the same property compose

Stacking two effects that touch the same property is legitimate, and it produces a **combined** result, not "the last one wins". That is the engine's contract, and it is not the one CSS animations have — hence this rule.

```json
"animation": [
  { "name": "fade_in", "duration": 0.6 },
  { "name": "pulse", "loop": true }
]
```

`fade_in` and `pulse` both touch `opacity`. At an instant where `fade_in` resolves to `0.5` and `pulse` to `0.8`, the rendered opacity is **0.40**, their product — not `0.8`.

## How each property combines

| Behaviour | Properties |
|---|---|
| **Product** | `opacity`, `scale_x`, `scale_y` |
| **Sum** | `translate_x`, `translate_y`, `rotation`, `rotate_x`, `rotate_y` |
| **Last value written** | everything else: `blur`, `blur_x`, `blur_y`, `color`, `border_radius`, `font_size`, `width`, `height`, `gap`, `padding`, `stroke_width`, `letter_spacing`, `draw_progress`, `draw_start`, … |

The grouping is by **effect family**, not by array entry: every preset resolves together, every `keyframes` entry resolves together, and the two results are then combined. Two presets animating `opacity` therefore multiply with each other before they ever get there.

## A neutral value is not "do nothing"

For a **composing** property, `1` (product) and `0` (sum) are the identity elements: applying them and skipping them give the same answer. Nothing subtle.

For a **last-value-written** property it is different: `blur: 0` is a value, not an absence. A second animation that takes the blur back to zero must clear the blur the first one set.

That is why those properties have a **negative** resting value (`-1`) rather than `0`: the engine tells "this animation did not touch the property" apart from "this animation took it to zero". A guard of the form `if other.blur > 0.001` conflates the two and leaves the element blurred for the rest of the scene — the bug issue #322 reported.

## If you really want only one to win

There is no keyword for it. Bound the windows so they do not overlap:

```json
"animation": [
  { "name": "fade_in", "delay": 0.0, "duration": 0.6 },
  { "name": "pulse", "delay": 0.6, "duration": 1.2, "loop": true }
]
```

Outside its own window an effect contributes nothing, so the question of composition no longer arises. See [animation-completion-budget.md](animation-completion-budget.md) for the window arithmetic.
