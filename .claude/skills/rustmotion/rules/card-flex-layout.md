# Rule: Use card/flex for Layout, Not group

`card` and `flex` (alias for `card`) use a CSS flexbox engine that auto-positions children. Use `card` for visual containers (background, border, shadow) and `flex` for pure layout.

Key patterns:
- **Horizontal row:** `"flex-direction": "row"` + `"gap"`
- **Vertical stack:** `"flex-direction": "column"` (default) + `"gap"`
- **Centered content:** `"align-items": "center"` + `"justify-content": "center"`
- **Auto-height:** `"size": { "width": 800, "height": "auto" }`
- **Grid:** `"display": "grid"` + `"grid-template-columns"`

Children WITHOUT a `position` field participate in flex flow. Children WITH a `position` field become absolutely positioned within the card.

**GOOD** (icon + text row):
```json
{
  "type": "card",
  "position": { "x": 140, "y": 800 },
  "size": { "width": 800, "height": "auto" },
  "style": {
    "flex-direction": "row",
    "align-items": "center",
    "gap": 16,
    "padding": 24,
    "background": "#1E293B",
    "border-radius": 16
  },
  "children": [
    { "type": "icon", "icon": "lucide:check-circle", "size": { "width": 48, "height": 48 }, "style": { "color": "#22C55E" } },
    { "type": "text", "content": "Feature enabled", "style": { "font-size": 32, "color": "#FFFFFF" } }
  ]
}
```
