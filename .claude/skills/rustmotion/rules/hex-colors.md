# Rule: Colors in Hex Format Only

**CRITICAL:** All color values MUST use hex format (`#RRGGBB` or `#RRGGBBAA`). NEVER use `rgba()`, `rgb()`, named colors, or CSS shorthand — rustmotion does not support CSS color functions. Using `rgba()` will render as **solid green blocks**.

**GOOD:**
```json
{ "color": "#FFFFFF" }
{ "color": "#00000080" }
{ "background": "#FFFFFF08" }
{ "border": { "color": "#FFFFFF14", "width": 1 } }
```

**BAD:**
```json
{ "color": "white" }
{ "color": "rgb(255, 255, 255)" }
{ "color": "rgba(255,255,255,0.03)" }
{ "color": "#FFF" }
```

## Common Hex Conversions for Glassmorphism

| Opacity | Alpha hex | Example (white)    | Example (purple #7C3AED) |
|---------|-----------|--------------------|-|
| 3%      | `08`      | `#FFFFFF08`        | `#7C3AED08` |
| 5%      | `0D`      | `#FFFFFF0D`        | `#7C3AED0D` |
| 6%      | `0F`      | `#FFFFFF0F`        | `#7C3AED0F` |
| 8%      | `14`      | `#FFFFFF14`        | `#7C3AED14` |
| 10%     | `1A`      | `#FFFFFF1A`        | `#7C3AED1A` |
| 12%     | `1F`      | `#FFFFFF1F`        | `#7C3AED1F` |
| 15%     | `26`      | `#FFFFFF26`        | `#7C3AED26` |
| 20%     | `33`      | `#FFFFFF33`        | `#7C3AED33` |
| 25%     | `40`      | `#FFFFFF40`        | `#7C3AED40` |
| 30%     | `4D`      | `#FFFFFF4D`        | `#7C3AED4D` |
| 50%     | `80`      | `#FFFFFF80`        | `#7C3AED80` |
| 0%      | `00`      | `#00000000` (transparent) | |

## Quick formula
Alpha hex = `round(opacity * 255)` converted to uppercase hex.
- 3% → `round(0.03 * 255)` = 8 → `08`
- 20% → `round(0.20 * 255)` = 51 → `33`
- 50% → `round(0.50 * 255)` = 128 → `80`
