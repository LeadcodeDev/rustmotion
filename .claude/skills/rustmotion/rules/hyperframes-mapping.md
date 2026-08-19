# Rule: Hyperframes → rustmotion mapping

If you're asked for an effect from the Hyperframes catalogue (or an effect described in that vocabulary — "streaming text", "number wheel", "badge pop"…), **look it up in this table before writing anything**. Half of these effects already exist under another name, and hand-rebuilding them gives a worse result the validator can't verify.

| Hyperframes | In rustmotion |
|---|---|
| Blur In | `style.animation: [{ "name": "char_blur_in", "granularity": "word" }]` |
| Staggered Fade Up | `char_blur_in` / `char_slide_up` + `direction`, `distance`, `scale_from` |
| Top Down Letters | `char_slide_up` with `"direction": "down"` |
| Text Stagger | `char_blur_in` (rise) + `shimmer` effect (sweep) on the same `text` |
| Number Pop In | `char_blur_in` with `"granularity": "char"`, `"scale_from": 0.82` |
| Streaming Text | `char_blur_in` with `jitter`/`seed`/`ink_from` — see [streaming-text.md](streaming-text.md) |
| Typewriter | `typewriter` preset + `text.caret` |
| Text State Swap | `text.states` + `text.swap` |
| Number Wheel | `number_wheel` component — see [number-wheel.md](number-wheel.md) |
| Badge Pop | `badge` + `style.animation: [{ "name": "pop_in" }]` |
| Success Check | `success_check` component |
| Simulated Cursor | `pointer` component — see [pointer-walkthrough.md](pointer-walkthrough.md) |
| Card Resize | `keyframes` on `width`/`height` — see [card-resize.md](card-resize.md) |
| Arc Motion Path | `motion_path` effect + `orient` — see [motion-path.md](motion-path.md) |
| SVG Line Draw Loader | `draw_in` / `stroke_reveal` preset on an `svg` |
| Dynamic Grid | `animated-background` preset `grid_lines` |
| Page Slide | `transition: { "type": "slide" }` |
| Chromatic Aberration Wipe | `transition: { "type": "chromatic_wipe" }` |

## Two naming traps

`cursor` is **not** a mouse pointer: it's a text caret (a blinking bar). The mouse pointer with its click ring is `pointer`.

`counter` is **not** a digit wheel: it interpolates a *value* and rewrites the number every frame, so the glyphs jump. `number_wheel` scrolls strips of digits, like a mechanical odometer. A count going from 0 to 30,222 → `counter`. A figure landing → `number_wheel`.
