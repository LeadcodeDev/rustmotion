# Stat / KPI Cards Best Practices

## Use `stat` for dashboard KPIs

The `stat` component combines value + label + trend + sparkline in one card. Always wrap with a background via `style.background` for visual separation.

## GOOD: Stat card with all features
```json
{
  "type": "stat",
  "value": "45.2K",
  "label": "Active Users",
  "trend": { "value": "+12.5%", "direction": "up" },
  "sparkline_data": [20, 25, 22, 30, 28, 35, 32, 40, 38, 45],
  "sparkline_color": "#22C55E",
  "size": { "width": 280, "height": 180 },
  "style": { "background": "#1E293B", "border-radius": 16 }
}
```

## Trend direction & color

- `"up"` → green arrow by default (positive metric)
- `"down"` → red arrow by default (negative metric)
- Override with `"color": "#22C55E"` when direction is good (e.g. churn **decreasing**)

## BAD: counter inside card for KPIs
```json
{ "type": "card", "children": [{ "type": "counter" }, { "type": "text" }] }
```
Counter has centering bugs inside cards. Use `stat` instead which handles layout internally.

## Dashboard layout pattern
Place 3-4 stat cards in a row with absolute positioning, 320px apart:
```json
{ "type": "stat", "position": "absolute", "x": 80, "y": 100, "size": { "width": 280, "height": 180 } },
{ "type": "stat", "position": "absolute", "x": 400, "y": 100, "size": { "width": 280, "height": 180 } },
{ "type": "stat", "position": "absolute", "x": 720, "y": 100, "size": { "width": 280, "height": 180 } }
```
