# Rule: Keep Content Inside the Viewport

No textual content may bleed out of the device viewport. The renderer enforces this through three opt-in mechanisms, all checked by `rustmotion validate`.

## 1. Text wrapping (`style.wrap`)

`text` is constraint-aware: it wraps to its parent's allocated width by default. Set `style.wrap: false` only when you intentionally want the text to render on one line and you guarantee it fits (e.g. a marquee that bleeds, a ticker, a title with a fixed `max-width`).

- Default: `wrap: true` (wraps at parent or `max_width`, whichever is smaller).
- `wrap: false` → validator fails with `unwrappable_text_overflow` if the natural width exceeds the box.

```json
{ "type": "text", "content": "Long sentence...", "style": { "wrap": true, "max-width": 800 } }
```

## 2. Codeblock / Terminal `auto_scroll`

When you give a `codeblock` or `terminal` a fixed `size` smaller than its natural content height, the renderer scrolls the content vertically (clip + translate) so the **last revealed line stays visible**. Font size is **never** reduced.

- Default: `auto_scroll: true`.
- `auto_scroll: false` → validator fails with `auto_scroll_disabled_overflow` if content doesn't fit.

```json
{
  "type": "codeblock",
  "auto_scroll": true,
  "code": "<long code, more lines than the box can hold>",
  "style": { "width": 1160, "height": 480 }
}
```

## 3. Container `style.overflow`

CSS-like semantics: `visible` (default) lets children bleed; `hidden` clips at the parent box. The validator only fails when content escapes the **viewport**, not a `visible` parent — a badge sticking out of a card is legal.

```json
{ "type": "card", "style": { "overflow": "hidden" } }
```

## What the validator catches

`rustmotion validate scenario.json` reports three geometry violation kinds:

- `viewport_overflow` — absolute bbox crosses the device edge
- `unwrappable_text_overflow` — `wrap: false` but natural width > available width
- `auto_scroll_disabled_overflow` — `auto_scroll: false` but content > box

`marquee` and `cursor` are exempt (their job is to bleed).

## CLI usage

```bash
rustmotion validate scenario.json                       # human-readable
rustmotion validate scenario.json --report report.json  # JSON report
rustmotion validate scenario.json --fix                 # safe auto-fixes
rustmotion validate scenario.json --strict-anim         # per-frame check
rustmotion validate scenario.json --lenient             # warnings only
```

`--fix` rewrites the file in place. It only applies *safe* mutations:
- sets `style.wrap: true` on text that overflowed because wrap was off
- sets `auto_scroll: true` on codeblock/terminal with overflow

Position/size clamping is never auto-applied — fix those by hand.

## When to use what

| Symptom | Fix |
|---|---|
| Long sentence cut at viewport edge | leave `wrap: true` (default) and ensure parent has finite width |
| Need a single-line title that must fit | set `max-width` and a small enough `font-size`, leave `wrap: true` |
| Marquee / ticker text that intentionally scrolls past edges | use `marquee` (exempt) — never `text` with `wrap: false` |
| Code listing taller than its box | leave `auto_scroll: true` (default) |
| Terminal log streaming many lines | leave `auto_scroll: true` |
| Badge protruding from a card on purpose | container has `overflow: visible` (default) — no change needed |
| Hard-clip children to a card border | container `style.overflow: "hidden"` |
