# Rule: Layer Order Matters (First = Behind)

Components in the `children` array render bottom-to-top: first element is the back layer, last element is the front layer. Place backgrounds and shapes before text and icons.

**GOOD:**
```json
"children": [
  { "type": "shape", "shape": "rounded_rect", "...": "(background, renders behind)" },
  { "type": "text", "content": "Visible on top", "...": "(renders in front)" }
]
```

**BAD:**
```json
"children": [
  { "type": "text", "content": "Hidden behind shape" },
  { "type": "shape", "shape": "rounded_rect", "...": "(covers the text)" }
]
```
