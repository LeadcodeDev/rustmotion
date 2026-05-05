# Rule: Stagger Animations with animation.delay

To create sequential entrance effects (items appearing one by one), use `animation.delay` with increasing values. Use 0.15–0.3s increments for smooth staggering.

**GOOD:**
```json
[
  { "type": "text", "content": "First",  "animation": [{ "name": "fade_in_up", "delay": 0.0, "duration": 0.6 }], "style": { "font-size": 48, "color": "#FFFFFF" } },
  { "type": "text", "content": "Second", "animation": [{ "name": "fade_in_up", "delay": 0.2, "duration": 0.6 }], "style": { "font-size": 48, "color": "#FFFFFF" } },
  { "type": "text", "content": "Third",  "animation": [{ "name": "fade_in_up", "delay": 0.4, "duration": 0.6 }], "style": { "font-size": 48, "color": "#FFFFFF" } }
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
