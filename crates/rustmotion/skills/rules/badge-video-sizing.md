# Rule: Badge Sizing for Video Resolution

`badge_size` (sm/md/lg) is designed for UI screen pixels. At video resolutions (1080×1920), all three sizes appear tiny. Use `style.font-size` to override.

## Size reference

| badge_size | font-size | Appears at 1080px |
|---|---|---|
| `sm` | 14px | Unreadable dot |
| `md` | 18px | Tiny pill |
| `lg` | 22px | Still small |
| `style.font-size: 32` | 32px | Readable on 1080px |
| `style.font-size: 40` | 40px | **Recommended for 1080×1920** |
| `style.font-size: 52` | 52px | Hero / large emphasis |

`style.font-size` overrides `badge_size`'s font. Padding and icon size scale proportionally via `resolved_params()`.

## Template — 1080×1920 badge

```json
{
  "type": "badge",
  "text": "Premier plan",
  "icon": "lucide:zap",
  "color": "#6366F1",
  "position": "absolute",
  "x": 270,
  "y": 580,
  "style": {
    "font-size": 40,
    "z-index": 2,
    "box-shadow": [{ "color": "#6366F1A0", "offset-y": 0, "blur": 60 }],
    "animation": [
      { "name": "scale_in", "delay": 0.4, "duration": 0.5 },
      { "name": "wiggle", "property": "translate_y", "amplitude": 11, "frequency": 1.2, "seed": 42 }
    ]
  }
}
```

## Positioning for portrait 1080×1920

The badge is typically placed above a card. If the card is vertically centered (~y=710 for a 500px-tall card), place the badge at `y = card_top - badge_height - gap`. With `font-size: 40`, badge height ≈ 80px. Example: card at y=720 → badge at y=620.

For a badge that spans most of the card width, place `x` so the badge pill center aligns with the card center (x = 1080/2 - badge_width/2). With font-size 40 and ~10 char text, width ≈ 280px → x ≈ 400.
