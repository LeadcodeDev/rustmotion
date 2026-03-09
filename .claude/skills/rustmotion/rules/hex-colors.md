# Rule: Colors in Hex Format Only

All color values must be hex: `#RRGGBB` or `#RRGGBBAA`. Named colors, CSS functions, and shorthand are NOT supported.

**GOOD:**
```json
{ "color": "#FFFFFF" }
{ "color": "#00000080" }
```

**BAD:**
```json
{ "color": "white" }
{ "color": "rgb(255, 255, 255)" }
{ "color": "#FFF" }
```
