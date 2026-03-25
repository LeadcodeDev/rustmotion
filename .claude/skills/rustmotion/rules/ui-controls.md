# UI Control Components

## Switch, Slider, Rating — animated interactive controls

These components simulate UI interactions with time-based animations.

### Switch
- `toggle_at` triggers the flip — the thumb slides with ease_out_cubic
- Always add a `label` for context

### Slider
- Set `value` for initial position, `animate_to` + `animate_at` for animation
- `show_value: true` displays the percentage above the thumb

### Rating
- Partial stars work: `value: 4.5` shows 4 full + half star
- Stars are 5-pointed paths, not icons — no Iconify dependency

## GOOD: Animated UI demo
```json
{ "type": "switch", "value": false, "toggle_at": 1.5, "label": "Dark Mode" },
{ "type": "slider", "value": 0.2, "animate_to": 0.8, "animate_at": 1.0, "show_value": true },
{ "type": "rating", "value": 4.5, "max": 5, "size": 32 }
```

## BAD: No animation timing
```json
{ "type": "switch", "value": true }
```
Without `toggle_at`, the switch is static — always set a toggle time for videos.
