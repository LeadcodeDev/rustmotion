# Rule: Grid Cards Need Explicit Container Height

Grid cells stretch to fill their row height. When the grid container has `height: "auto"`, rows expand to consume all available space from the parent flex layout, making cards much taller than their content.

**Always set an explicit height on grid containers** and use `grid-template-rows` to control row sizes.

**GOOD:**
```json
{
  "type": "card",
  "style": {
    "width": 1200,
    "height": 400,
    "display": "grid",
    "grid-template-columns": ["1fr", "1fr", "1fr"],
    "grid-template-rows": ["1fr", "1fr"],
    "gap": 24
  },
  "children": [ ... ]
}
```

**BAD** (cards stretch to fill scene height):
```json
{
  "type": "card",
  "style": {
    "width": 1200,
    "height": "auto",
    "display": "grid",
    "grid-template-columns": ["1fr", "1fr", "1fr"],
    "gap": 24
  },
  "children": [ ... ]
}
```

## Why This Happens

1. Scene flex layout gives the grid container all remaining vertical space
2. Without `grid-template-rows`, rows default to equal shares of that space
3. Grid cells stretch to fill their assigned row height
4. Child `height: "auto"` is ignored — the grid cell size wins

## Tips

- Calculate container height: `(row_count × estimated_card_height) + ((row_count - 1) × gap) + (padding × 2)`
- Use `grid-template-rows` with `"1fr"` entries to distribute rows evenly within the explicit height
- For single-row grids, a flex row with `"flex-direction": "row"` may be simpler
