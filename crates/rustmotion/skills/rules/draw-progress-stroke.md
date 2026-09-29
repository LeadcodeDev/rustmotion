# Rule: `draw_progress` — nothing at 0, and a stroke that does not change appearance as it finishes

`draw_progress` reveals a stroke progressively. It is driven by a `draw_in`/`stroke_reveal` preset, or by `keyframes` on the property of the same name. Three traps to know about on `line` and `svg` (`reveal: "stroke"`, the default).

## `svg`: `draw: true` is not a driver

`draw: true` forces the draw-on **render path**. It does not advance `draw_progress`. With no driver the property stays at its resting value, the painter takes the "finished" branch (`progress >= 1.0`, which simply delegates to resvg) and the mark renders **exactly as it would with `draw: false`** — verified byte for byte on two PNGs.

The validator therefore rejects `draw: true` without a driver, rather than letting the flag look as though it did something:

```
draw: true but nothing animates draw_progress — the mark renders finished,
pixel-identical to draw: false. Add a 'draw_in' or 'stroke_reveal' preset,
or keyframes on 'draw_progress'.
```

In practice `draw: true` is not needed at all: a `draw_in` preset is enough on its own, since the painter switches as soon as `draw_progress` is inside `[0, 1)`.

## `line`: `draw_progress: 0` must paint nothing

`Line::paint` forces a round cap (`PaintCap::Round`) and builds a dash pattern `[drawn_length, remainder]` to reveal the stroke. At `draw_progress: 0`, `drawn_length` is `0` — and a zero-length dash with a round cap still paints: Skia draws a solid dot of diameter `width`, exactly at the start point. The component now returns without painting anything as soon as `draw_progress <= 0` (within the `[0, 1)` window — an absent `draw_progress`, or one `>= 1`, still means the whole stroke, unchanged).

## An `svg` in the middle of drawing itself must look like the finished stroke

While tracing (`paint_draw_on`, `draw_progress` in `(0, 1)`), the stroke must have the **same** width, the same `stroke-linecap` and the same `stroke-linejoin` as the final render (`progress >= 1`, painted by resvg) — otherwise the last frame of the trace and the first "finished" frame do not join up visually (a jump in width, a cap appearing out of nowhere).

Concretely:

- The canvas is already scaled from the `viewBox` to the node's size (`canvas.scale((scale_x, scale_y))`) before each segment is painted: the source SVG's `stroke-width` must be set on the `Paint` as-is, with no further compensation. Dividing by the scale factor undoes that scaling and pins the stroke to its raw SVG width whatever the node's size — the bug a future rework must not reintroduce.
- The source `<path>`'s `stroke-linecap`/`stroke-linejoin` (read from `usvg::Stroke`) must be set on each segment's `Paint`, not merely used for the final render. A `round` cap on the finished stroke but `butt` (Skia's default) during the trace makes the cap appear all at once at `draw_progress = 1`, with a visible extension of the stroke (the cap's radius).

`marquee` and `cursor` are out of scope here: they are not strokes revealed by `draw_progress`.
