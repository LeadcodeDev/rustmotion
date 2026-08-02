# Rule: Grid Containers — `height: "auto"` Sizes to Content (Verified)

**Update:** the stretching behavior this rule used to warn about is **not reproducible on the current build**. Rendered directly: a `card` with `display: "grid"`, `height: "auto"`, `grid-template-columns: ["1fr","1fr","1fr"]`, and **no** `grid-template-rows` — with children that have no explicit height either — produces a grid correctly sized to its content (each row matches its tallest cell), not a grid stretched to fill the scene. Same result whether the cell children carry explicit heights or are left to their own intrinsic size. If you hit stretching on a specific scenario, treat it as a new bug to investigate rather than expected behavior — it doesn't reproduce from `height: "auto"` alone.

**`height: "auto"` on a grid container is safe** — you do not need an explicit height just to prevent stretching.

**GOOD — auto height, content-sized rows (no `grid-template-rows` needed for this):**
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
  "children": [
    { "type": "card", "style": { "background": "#1E293B", "border-radius": 12, "padding": 24 }, "children": [{ "type": "text", "content": "A", "style": { "font-size": 32, "color": "#FFFFFF" } }] },
    { "type": "card", "style": { "background": "#1E293B", "border-radius": 12, "padding": 24 }, "children": [{ "type": "text", "content": "B", "style": { "font-size": 32, "color": "#FFFFFF" } }] },
    { "type": "card", "style": { "background": "#1E293B", "border-radius": 12, "padding": 24 }, "children": [{ "type": "text", "content": "C", "style": { "font-size": 32, "color": "#FFFFFF" } }] }
  ]
}
```

## When `grid-template-rows` is still useful

Not to prevent stretching (auto-height already avoids that), but when you want **explicit control** over row proportions instead of each row sizing independently to its tallest cell:
- Forcing all rows to the *same* height regardless of content (e.g. `grid-template-rows: ["1fr", "1fr"]` on an explicit-height container so two rows split evenly even if one row's content is shorter).
- A fixed pixel height for a specific row (e.g. `["200px", "auto"]`).

## Tips

- If you *do* set an explicit `height` on the grid container (e.g. to force uniform row heights), calculate it as: `(row_count × target_row_height) + ((row_count - 1) × gap) + (padding × 2)`.
- Grid cells still need sizeable content: the 23 component types with no intrinsic measurer (`stat`, `chart`, `gauge`, `sparkline`, etc. — see [card-flex-layout.md](card-flex-layout.md)) need their own explicit `width`/`height` regardless of the grid container's height mode, or they render at 0×0 inside their cell.
- For single-row grids, a flex row with `"flex-direction": "row"` may be simpler.
