# Rule: text-background Renders Behind Text

`text-background` adds a colored rectangle behind text content. The background is positioned to tightly wrap the text glyphs with configurable padding and corner radius.

`text-background` is a **root field** on `text` — a sibling of `style`, not nested inside it. `CssStyle` has no `text-background` key (`deny_unknown_fields`); putting it in `style` silently drops the whole `text` component.

**GOOD:**
```json
{
  "type": "text",
  "content": " Highlighted text ",
  "text-background": {
    "color": "#6366F1",
    "padding": 12,
    "corner_radius": 8
  },
  "style": {
    "font-size": 36,
    "color": "#FFFFFF",
    "text-align": "center"
  }
}
```

## Fields

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `color` | string | required | Background color (hex) |
| `padding` | f32 | `4` | Padding around text |
| `corner_radius` | f32 | `4` | Rounded corners radius (0 = square) |

## Tips

- Add spaces around `content` (e.g. `" text "`) for visual breathing room beyond padding
- Works with multi-line text: each line gets its own background rectangle
- Combines with other text styles (`text-shadow`, `stroke`, etc.)
