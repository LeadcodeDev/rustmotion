# Rule: The four text finishes

Four mechanisms that each used to demand a hand-assembled sub-tree now write in one line. None of them replaces an entry preset: they layer on top.

## `shimmer` — the light sweeps over the letters

An animation effect, not a component field. The band only lights up **pixels that are actually painted** (composited `SrcATop` inside the node's layer): on a `text`, the light catches the glyphs, not the box.

```json
"animation": [{
  "name": "shimmer",
  "delay": 1.0, "duration": 1.1,
  "color": "#7DD3FC", "intensity": 0.85,
  "width": 0.3, "angle": 22, "loop": true
}]
```

`width` is the band's width as a fraction of the sweep (0.3 = a sharp glint, 0.8 = a soft wash). `angle` tilts the band: `0` is vertical and sweeps left to right; ~20° is what makes it read as a reflection rather than a wipe. The isolated layer is only opened during the effect's window — an unlooped `shimmer` costs nothing for the rest of the scene.

Combined with `char_blur_in` on the same `text`, this reproduces "text stagger": the words rise while unblurring, then the light sweeps through.

## `text.states` — a label that becomes another

```json
{
  "type": "text",
  "content": "Saving draft",
  "states": [{ "at": 2.6, "content": "Saved" }],
  "swap": { "duration": 0.45, "distance": 22, "blur": 9 },
  "style": { "font-size": 52, "white-space": "nowrap", "max-width": "600px" }
}
```

Without `swap`, labels cut sharply at each `at` — abrupt, but that's exactly what omitting the field asks for. With `swap`, both are on screen during the window: the outgoing one rises while blurring, the incoming one rises from below while unblurring.

**The box is measured on the longest label**, not the first one. A box sized for `"Saved"` would overflow the moment it returns to `"Saving draft"` — and the validator would have caught it.

## `text.caret` — the caret follows the reveal

```json
{ "type": "text", "content": "rustmotion --frames 0-60",
  "caret": { "shape": "block", "blink": 0.9, "color": "#38BDF8" },
  "style": { "animation": [{ "name": "typewriter", "duration": 2.0 }] } }
```

`shape`: `line` (thin rule) or `block` (terminal-style). `blink` is the full period in seconds (`0` = fixed). `hide_when_done: true` removes the caret once the reveal is finished instead of leaving it parked.

That's the field's reason to exist: a `cursor` composited next to the text would stay where it was placed while the text grows underneath it. The caret is also present **before** the first character, otherwise the first frame is empty and then caret and letter appear together, which reads as a glitch.

## `pop_in` — the badge arrival

An animation preset: the element grows from nothing with a `back-out` overshoot, **then** a short elastic pulse once it settles. `overshoot` sets the pulse's amplitude (default 0.18 = 118%); `0` removes it and leaves a plain scale-in.

Both beats matter: the first *places* the element, the second draws the eye back to it. Merged into a single curve, they'd read as a tremor.
