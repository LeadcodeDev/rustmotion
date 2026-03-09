# Rule: Counter Must Be Standalone

`counter` renders correctly only as a standalone root-level scene child. Inside a `card`, there is no baseline correction — the counter renders from the top of the layout box and looks misaligned.

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
  "position": { "x": 540, "y": 960 },
  "start_at": 0.5,
  "end_at": 2.5,
  "easing": "ease_out",
  "style": { "font-size": 72, "color": "#FFFFFF", "font-weight": "bold", "text-align": "center" }
}
```
