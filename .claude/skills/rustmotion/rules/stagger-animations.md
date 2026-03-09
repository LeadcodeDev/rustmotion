# Rule: Stagger Animations with preset_config.delay

To create sequential entrance effects (items appearing one by one), use `preset_config.delay` with increasing values. Use 0.15–0.3s increments for smooth staggering.

**GOOD:**
```json
[
  { "type": "text", "content": "First",  "position": { "x": 540, "y": 400 }, "preset": "fade_in_up", "preset_config": { "delay": 0.0, "duration": 0.6 }, "style": { "font-size": 48, "color": "#FFFFFF" } },
  { "type": "text", "content": "Second", "position": { "x": 540, "y": 500 }, "preset": "fade_in_up", "preset_config": { "delay": 0.2, "duration": 0.6 }, "style": { "font-size": 48, "color": "#FFFFFF" } },
  { "type": "text", "content": "Third",  "position": { "x": 540, "y": 600 }, "preset": "fade_in_up", "preset_config": { "delay": 0.4, "duration": 0.6 }, "style": { "font-size": 48, "color": "#FFFFFF" } }
]
```

For cards with `stagger`, use the `stagger` property instead:
```json
{
  "type": "card",
  "stagger": 0.15,
  "children": [ ... ]
}
```
