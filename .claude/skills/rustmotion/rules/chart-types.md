# Chart Type Selection Guide

## When to use each chart type

| Chart Type | Best For | Data Shape |
| --- | --- | --- |
| `bar` | Comparing categories | `data: [{ value, label }]` |
| `horizontal_bar` | Rankings, long labels | `data: [{ value, label }]` + `show_labels: true` |
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

## Axes & grid

Add `show_grid: true`, `show_x_labels: true`, `show_y_labels: true` to cartesian charts (bar, line, area, stacked_bar, scatter, waterfall) for professional appearance. Customize with `grid_color`, `label_color`, `label_font_size`.

## BAD: using pie for too many items
```json
{ "chart_type": "pie", "data": [8 items...] }
```

## GOOD: use horizontal_bar for rankings
```json
{ "chart_type": "horizontal_bar", "data": [...], "show_labels": true }
```
