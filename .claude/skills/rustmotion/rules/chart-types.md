# Chart Type Selection Guide

## When to use each chart type

| Chart Type | Best For | Data Shape |
| --- | --- | --- |
| `bar` | Comparing categories | `data: [{ value, label }]` |
| `horizontal_bar` | Rankings, long labels | `data: [{ value, label }]` + `show_y_labels: true` |
| `line` | Trends over time | `data: [{ value, label }]` |
| `area` | Trends with volume emphasis | `data: [{ value }]` + `smooth: true`, `fill_opacity` |
| `pie` | Part-of-whole (max 6 slices) | `data: [{ value, label, color }]` |
| `donut` | Part-of-whole with center stat | Same as pie + `inner_radius: 0.6` |
| `stacked_bar` | Multi-series comparison | `categories` + `series: [{ name, data, color }]` |
| `radar` | Multi-axis comparison | `axes` + `radar_data: [{ values, color }]` |
| `scatter` | Correlation between 2 variables | `points: [{ x, y, size, color }]` |
| `radial_bar` | Progress of multiple items | `data: [{ value, label, color }]` (Apple Health style) |
| `funnel` | Conversion pipeline | `data` (descending values) + `direction: "horizontal"/"vertical"` |
| `waterfall` | Cumulative changes | `data` (positive = green, negative = red) |

## A chart draws NO text unless you ask for it

`show_grid`, `show_x_labels`, `show_y_labels` and `show_labels` all default to
**`false`**. A chart written as `{ "type": "chart", "chart_type": "bar", "data":
[...] }` renders coloured shapes and nothing else — no axis, no numbers, no
category names. `radar` is the only exception: it always draws its axis labels
and its grid, and ignores the four flags entirely.

Which flag produces text, per type — measured, not assumed:

| Type | `show_grid` | `show_y_labels` | `show_x_labels` | `show_labels` |
| --- | --- | --- | --- | --- |
| `bar` | grid lines | value ticks (left) | category names (below) | — |
| `stacked_bar` | grid lines | value ticks (left) | category names (below) | — |
| `waterfall` | grid lines | value ticks (left) | category names (below) | — |
| `line` / `area` | grid lines | value ticks (left) | point labels (below) | — |
| `scatter` | grid lines | y value ticks | x value ticks | — |
| `horizontal_bar` | grid lines (vertical) | category names (left gutter) | value ticks (below) | label in bar + value at row end |
| `funnel` | — | — | — | label inside each segment |
| `pie` / `donut` / `radial_bar` | — | — | — | — |

`pie`, `donut` and `radial_bar` have **no label support at all**: all four flags
are accepted by the schema and do nothing. If the viewer needs to know which
slice or ring is which, put the legend next to the chart yourself (a `flex` of
`badge` or coloured `shape` + `text` rows), or use `bar` / `horizontal_bar`
instead.

## Text size

`label_font_size` defaults to **12px**, which is below the project's legibility
floor of 1.2% of output height (≈13px on 1080p, ≈26px on 4K) — and the floor
check does **not** cover chart text, so `validate` will not warn you. On a 1080p
canvas set `label_font_size` to at least 14; scale it with the canvas above that.

## `horizontal_bar`: use the left gutter for names

`show_y_labels: true` puts the category names in a left gutter sized to the
longest one — the standard ranking layout, and the only option that stays
legible when a bar is short or zero. `show_labels: true` instead writes the name
inside the bar (falling back to just past its end when it doesn't fit) and adds
the value, right-aligned at the end of the row.

Every row also gets a faint full-width track, so a zero-valued row still occupies
its slot instead of vanishing.

```json
{
  "type": "chart", "chart_type": "horizontal_bar",
  "data": [
    { "label": "Native TypeScript", "value": 6 },
    { "label": "Effect", "value": 0 }
  ],
  "show_y_labels": true, "show_grid": true, "show_x_labels": true,
  "show_labels": true, "label_font_size": 16,
  "style": { "width": 1440, "height": 400 }
}
```

## Degenerate inputs

| Input | What renders |
| --- | --- |
| A zero value | Its slot is kept: bar/horizontal_bar keep the label and (horizontal_bar) the track; a zero pie slice is skipped |
| All values zero | `pie`/`donut` render nothing at all (there is no whole to be part of); cartesian types still draw axes and labels |
| Empty `data` | Nothing renders, and `validate` does **not** complain. Check your data is non-empty before you ship |
| A single point | `bar`/`pie`/`donut` fill the chart; `line`/`area` draw the axes plus one dot — prefer a `stat` or `counter` for one number |
| All values equal | `bar` fills every bar; `line`/`area`/`scatter` centre the flat series vertically |
| Negative values | `bar`/`horizontal_bar`/`waterfall`/`line`/`area` anchor the axis at zero and draw negatives the other side of it. `pie`/`donut`/`radial_bar`/`funnel` clamp them to zero — a negative share is meaningless there |
| More categories than fit | Nothing is dropped and nothing overflows the box, but the x labels collide as soon as the widest one exceeds the slot width (`chart_width / n`) — 30 four-character labels in an 800px-wide `bar` measured as an unreadable band. Use `horizontal_bar` (which stacks names down the gutter) or cut the data |
| Long category labels | `horizontal_bar` gutter grows to fit, capped at 45% of the width; `bar` x labels will overlap each other. `radar` anchors long axis labels inward so they stay in the box |

## BAD: using pie for too many items

```json
{ "chart_type": "pie", "data": [8 items...] }
```

Eight slices and no label support means eight anonymous colours.

## GOOD: use horizontal_bar for rankings

```json
{ "chart_type": "horizontal_bar", "data": [...], "show_y_labels": true, "show_labels": true }
```
