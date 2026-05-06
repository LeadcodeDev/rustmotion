# Rule: Use card/flex/div for Layout

`card` and `flex` (alias for `card`) use a CSS flexbox engine that auto-positions children. Use `card` for visual containers (background, border, shadow), `div` for invisible grouping and pure layout (no background, no border, no clipping).

## Scene = Implicit Flex Container

Every scene acts as an implicit full-screen flex container (`direction: column` by default). Children without `position` participate in flex flow automatically. Children with `position` are absolutely positioned.

You can customize the scene layout:
```json
{
  "duration": 5.0,
  "layout": {
    "direction": "column",
    "align_items": "center",
    "justify_content": "center",
    "gap": 24,
    "padding": 40
  },
  "children": [
    { "type": "text", "content": "Centered title", "style": { "font-size": 64, "color": "#FFFFFF" } },
    { "type": "text", "content": "Subtitle below", "style": { "font-size": 32, "color": "#94A3B8" } }
  ]
}
```

## Card/Flex Patterns

Key patterns:
- **Horizontal row:** `"flex-direction": "row"` + `"gap"`
- **Vertical stack:** `"flex-direction": "column"` (default) + `"gap"`
- **Centered content:** `"align-items": "center"` + `"justify-content": "center"`
- **Auto-height:** `"style": { "width": 800, "height": "auto" }`
- **Grid:** `"display": "grid"` + `"grid-template-columns"`

Children flow in the flexbox. Use `positioned` container for absolute positioning.

**Grid height warning:** Grid containers need an explicit `height` (not `"auto"`) to prevent rows from stretching to fill all available space. See [rules/grid-card-height.md](rules/grid-card-height.md).

**GOOD** (icon + text row):
```json
{
  "type": "card",
  "style": {
    "width": 800,
    "height": "auto",
    "flex-direction": "row",
    "align-items": "center",
    "gap": 16,
    "padding": 24,
    "background": "#1E293B",
    "border-radius": 16
  },
  "children": [
    { "type": "icon", "icon": "lucide:check-circle", "style": { "width": 48, "height": 48, "color": "#22C55E" } },
    { "type": "text", "content": "Feature enabled", "style": { "font-size": 32, "color": "#FFFFFF" } }
  ]
}
```
