# Rule: Simulated mouse pointer (`pointer`)

For a product demo or an agent walkthrough — the arrow that moves to a control and clicks it — use `pointer`.

**`cursor` is not that.** `cursor` is a text caret: a blinking vertical bar. Its `cursor_style: "pointer"` field is dead metadata — it draws a bar either way.

```json
{
  "type": "pointer",
  "position": "absolute",
  "x": 0,
  "y": 0,
  "size": 52,
  "tone": "light",
  "click_ring": "bold",
  "ring_color": "#38BDF8",
  "click_duration": 0.5,
  "path": [
    { "time": 0.4, "x": 1500, "y": 820 },
    { "time": 2.0, "x": 480,  "y": 330 },
    { "time": 3.6, "x": 900,  "y": 690 }
  ]
}
```

| Field | Role |
|---|---|
| `size` | Height of the arrow in px. The click ring scales with it. |
| `tone` | `light` (white arrow, dark outline) or `dark` |
| `color` / `outline_color` | Override `tone` |
| `click_ring` | `subtle` / `standard` / `bold` / `none` |
| `path` | Waypoints `{time, x, y}` — the pointer **clicks on arrival** at each one |
| `click_at` | Clicks for a stationary pointer. **Ignored if `path` is present** |
| `click_duration` | Duration of the click, *and* the pause on the waypoint before moving on |
| `path_easing` | `ease_in_out` (default), `linear`, `ease_out`, `step` |

## Coordinates are relative to the component's own origin

A waypoint's `x`/`y` are relative to the `pointer`'s box, not to the device. Place the component with `position: absolute, x: 0, y: 0` and the waypoints then read as scene coordinates — that's the form to prefer for a walkthrough.

## The box is the glyph, not the path

The component's box is the size of the arrow: the waypoints translate it. Sizing the box to the path would push a `flex` sibling around because of an element that's just a cursor.

Corollary: `pointer` is **exempt from the viewport overflow check**, like `marquee` and `cursor`. A demo that brings the arrow near an edge legitimately puts its tail off-screen.

## The move pauses on the click

Between two waypoints, the pointer doesn't set off again until the click animation is done (`click_duration`). That's what makes the gesture read: arrive, click, leave. A `click_duration` close to the gap between two waypoints barely leaves time for the travel — leave at least double.
