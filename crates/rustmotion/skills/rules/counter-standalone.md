# Rule: Counter Centers Correctly — Size Its Parent for the Worst-Case Digit Width

`counter` centers correctly whether it is a standalone scene child **or** nested inside a `card`. Verified by rendering both cases: with `text-align: center`, the counter's ink centre lands exactly on the content-box centre in both contexts, using the same paint-time centering math (`counter.rs`) regardless of parent — there is no separate "standalone" code path and no missing baseline correction inside cards.

## The real constraint: the box never shrinks to fit

`CounterIntrinsic` reserves space for the natural (unwrapped) width of the *largest absolute value* between `from` and `to` (worst-case digits) — and, because the counter is atomic (`wrap: false`), that measurement **ignores** any known/definite width the parent offers; it always requests its full natural width. So if the parent (a `card`, or any fixed-width container) is narrower than that worst-case width, the counter's box is still sized to its content and **overflows the parent on both sides**.

**This is validator-silent when the parent uses the default `overflow: visible` and the overflow stays inside the video viewport** — `rustmotion validate`'s geometry pass only flags content that leaves the *viewport*, not a parent container (see `CLAUDE.md`'s "Sécurité géométrique" section). Confirmed by rendering: a counter needing up to 7 digits inside a 300px-wide card visibly spills past both edges of the card, while `rustmotion validate` reports `Valid scenario` with zero geometry violations — only a schema-level warning ("display width changes from N to M chars — ensure the parent container is at least wide enough").

So: size the parent to the counter's worst-case digit width (or wider), and don't rely on `rustmotion validate` to catch it if you don't.

**BAD** (card narrower than the counter's worst-case width — overflows silently, `to: 9_999_999` needs 7 digits' worth of width):
```json
{
  "type": "card",
  "style": { "width": 300, "height": "auto", "background": "#1E293B", "padding": 20 },
  "children": [
    { "type": "counter", "from": 0, "to": 9999999, "style": { "font-size": 72, "color": "#FFFFFF", "font-weight": "bold", "text-align": "center" } }
  ]
}
```

**GOOD** (standalone — no parent width constraint):
```json
{
  "type": "counter",
  "from": 0,
  "to": 100,
  "start_at": 0.5,
  "end_at": 2.5,
  "easing": "ease_out",
  "style": { "font-size": 72, "color": "#FFFFFF", "font-weight": "bold", "text-align": "center" }
}
```

**GOOD** (inside a card — fine, as long as the card is wide enough for the worst-case digit width):
```json
{
  "type": "card",
  "style": { "width": 700, "height": "auto", "background": "#1E293B", "padding": 20 },
  "children": [
    { "type": "counter", "from": 0, "to": 9999999, "style": { "font-size": 72, "color": "#FFFFFF", "font-weight": "bold", "text-align": "center", "width": "100%" } }
  ]
}
```

`end_at` is a visibility toggle, not an animation-completion boundary — setting it on a `counter` makes the number **disappear** once that time passes (the counter's own animation is driven by `ctx.time / scene_duration`, not by `start_at`/`end_at`). Use `start_at` only to delay the count-up.
