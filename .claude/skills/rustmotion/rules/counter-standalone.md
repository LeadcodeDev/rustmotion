# Rule: Counter Must Be Standalone

`counter` renders correctly only as a standalone root-level scene child. Inside a `card`, there is no baseline correction — the counter renders from the top of the layout box and looks misaligned.

`Counter::measure()` reserves space for the *largest absolute value* between `from` and `to` (worst-case digits). The layout never reflows mid-animation — so the parent must be wide enough to fit that worst-case width or the geometry validator fails with `viewport_overflow`.

**BAD:**
```json
{
  "type": "card",
  "children": [
    { "type": "counter", "from": 0, "to": 100 }
  ]
}
```

**GOOD:**
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
