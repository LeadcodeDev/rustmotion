# Rule: Resize a card (don't scale it)

A compact card growing into a detail panel is a **layout** change: its new box reflows its content. A `scale` stretches the pixels it already had, text included — that's a zoom, not a resize, and it shows immediately as blurred, oversized text.

Animate `width` / `height` with a `keyframes` effect: they reach taffy, so layout is recomputed every frame.

```json
{
  "type": "card",
  "style": {
    "width": "330px",
    "height": "132px",
    "background": "#111C33",
    "border-radius": 20,
    "justify-content": "center",
    "align-items": "center",
    "animation": [{
      "name": "keyframes",
      "delay": 1.2,
      "duration": 0.9,
      "keyframes": [
        { "property": "width",  "easing": "ease_out_cubic",
          "keyframes": [{ "time": 0.0, "value": 330 }, { "time": 0.9, "value": 620 }] },
        { "property": "height", "easing": "ease_out_cubic",
          "keyframes": [{ "time": 0.0, "value": 132 }, { "time": 0.9, "value": 240 }] }
      ]
    }]
  },
  "children": [ { "type": "text", "content": "…" } ]
}
```

## Keyframe times are relative to `delay`

`{"time": 0.0}` is the start of the effect, not the start of the scene. The effect's `delay` shifts the whole track.

## Animated size wins over intrinsic size

A component that declares its own size (a `shape`, a `badge`) has it overridden for the duration of the animation. That's intended, but it also means a value left at 0 on the last keyframe makes the box disappear.

## Make the content actually follow

Without `justify-content` / `align-items`, the child stays pinned to the top-left and only the box grows: the motion reads as empty. Centre the content (or give it `flex: 1`) so the growth actually reads.

## The validator sees the final box

`validate --strict-anim` samples the animation: a card growing past the device edge is flagged the instant it happens. Check that there's still margin at the maximum size, not just at the initial one.
