# Rule: Never Use group/stack for Layout

`group`/`stack` has a known bug: `Stack` does not override `Widget::layout()`, so it returns a `LayoutNode` with no children. Children inside a group/stack will **NOT render**.

**FORBIDDEN:** `"type": "group"` and `"type": "stack"`.

**BAD:**
```json
{
  "type": "group",
  "position": { "x": 100, "y": 100 },
  "children": [
    { "type": "shape", "shape": "circle", "size": { "width": 64, "height": 64 }, "style": { "fill": "#FF0000" } },
    { "type": "icon", "icon": "lucide:star", "size": { "width": 32, "height": 32 } }
  ]
}
```

**GOOD** (standalone elements with explicit positions):
```json
[
  { "type": "shape", "shape": "circle", "position": { "x": 100, "y": 100 }, "size": { "width": 64, "height": 64 }, "style": { "fill": "#FF0000" } },
  { "type": "icon", "icon": "lucide:star", "position": { "x": 116, "y": 116 }, "size": { "width": 32, "height": 32 }, "style": { "color": "#FFFFFF" } }
]
```

**GOOD** (use card/flex for layout):
```json
{
  "type": "card",
  "position": { "x": 100, "y": 100 },
  "size": { "width": 64, "height": 64 },
  "style": { "align-items": "center", "justify-content": "center" },
  "children": [
    { "type": "icon", "icon": "lucide:star", "size": { "width": 32, "height": 32 }, "style": { "color": "#FFFFFF" } }
  ]
}
```
