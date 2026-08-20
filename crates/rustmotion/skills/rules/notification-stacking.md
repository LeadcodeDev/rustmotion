# Notification Stacking

## How to stack multiple notifications

Notifications appear via fade-in and push existing ones down when a new one arrives.

### Pattern: 2 notifications
```json
{
  "type": "notification",
  "title": "First Alert",
  "variant": "success",
  "slide_in_at": 0.5,
  "push_at": [1.5],
  "position": "absolute", "x": 100, "y": 100
},
{
  "type": "notification",
  "title": "Second Alert",
  "variant": "warning",
  "slide_in_at": 1.5,
  "wait_for_push": true,
  "position": "absolute", "x": 100, "y": 100
}
```

### Rules
1. All stacked notifications share the **same x/y** position (top anchor)
2. Earlier notifications get `push_at: [time_of_next]` — timestamps when they shift down
3. Later notifications get `wait_for_push: true` — delays fade-in until push animation completes
4. For 3+ notifications, chain: first gets `push_at: [1.5, 3.0]`, second gets `push_at: [3.0]`, third gets `wait_for_push: true`
5. `slide_out_at` controls when each notification fades out independently

### Variant colors
- `info` → #3B82F6 (blue)
- `success` → #22C55E (green)
- `warning` → #F59E0B (amber)
- `error` → #EF4444 (red)
