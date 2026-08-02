# Stat / KPI Cards Best Practices

## `stat` requires explicit `width`/`height` — it has no intrinsic size

`stat` has **no intrinsic sizing** (verified against `crates/rustmotion-components/src/box_builder.rs`: `Stat` is absent from both `component_intrinsic` and `apply_intrinsic_overrides`, unlike `counter`/`text`/`badge`). Without an explicit `style.width`/`style.height`, a `stat` in a flex/grid layout lays out at 0×0 and paints nothing.

**Measured:** three `stat`s placed in a flex-row card with no explicit size render **zero pixels** — not a smaller-than-expected card, literally nothing on screen. Always set `style.width`/`style.height` explicitly:

## GOOD: Stat card with all features
```json
{
  "type": "stat",
  "value": "45.2K",
  "label": "Active Users",
  "trend": { "value": "+12.5%", "direction": "up" },
  "sparkline_data": [20, 25, 22, 30, 28, 35, 32, 40, 38, 45],
  "sparkline_color": "#22C55E",
  "style": { "width": 280, "height": 180, "background": "#1E293B", "border-radius": 16 }
}
```
This only works because `width`/`height` are set explicitly — that is a **requirement**, not a stylistic choice.

## BAD: stat with no explicit size in a flex row — renders nothing
```json
{
  "type": "card",
  "style": { "flex-direction": "row", "gap": 24 },
  "children": [
    { "type": "stat", "value": "45.2K", "label": "Active Users", "style": { "background": "#1E293B" } },
    { "type": "stat", "value": "12.8%", "label": "Growth", "style": { "background": "#1E293B" } },
    { "type": "stat", "value": "3.4s", "label": "Load time", "style": { "background": "#1E293B" } }
  ]
}
```
No `width`/`height` on any `stat` → all three collapse to 0×0. `rustmotion validate` does not catch this (it's a zero-size box, not an overflow) — the scene simply renders blank where the stats should be.

## Trend direction & color

- `"up"` → green arrow by default (positive metric)
- `"down"` → red arrow by default (negative metric)
- Override with `"color": "#22C55E"` when direction is good (e.g. churn **decreasing**)

## `stat` vs `counter` — they are not interchangeable

- `counter` **animates** its number over time (`from` → `to`, with `easing`). It centers correctly inside a card — verified: with `text-align: center`, its ink centre lands exactly on the content-box centre, both standalone and inside a card. The one real constraint is sizing the parent to its worst-case digit width (see [counter-standalone.md](counter-standalone.md)) — not centering.
- `stat` is a **static** composite (`value` is a plain string — there is no animated count-up). Use it when you want value + label + trend arrow + sparkline bundled in one box and don't need the number itself to animate. Use `counter` + `text` when the number needs to count up/down, or when you don't need trend/sparkline.

Neither is a drop-in replacement for the other; pick based on whether the number animates and whether you need the trend/sparkline extras.

## Dashboard layout pattern
Place 3-4 stat cards in a row with absolute positioning, 320px apart — each still needs its own explicit size:
```json
{ "type": "stat", "value": "45.2K", "label": "Active Users", "position": "absolute", "x": 80, "y": 100, "style": { "width": 280, "height": 180, "background": "#1E293B" } },
{ "type": "stat", "value": "12.8%", "label": "Growth", "position": "absolute", "x": 400, "y": 100, "style": { "width": 280, "height": 180, "background": "#1E293B" } },
{ "type": "stat", "value": "3.4s", "label": "Load time", "position": "absolute", "x": 720, "y": 100, "style": { "width": 280, "height": 180, "background": "#1E293B" } }
```
