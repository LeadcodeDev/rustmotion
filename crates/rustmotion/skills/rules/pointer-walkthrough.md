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
| `glyph` | `arrow` (default), `hand`, or `grab` — which shape is drawn |
| `click_glyph` | Glyph shown for the duration of a click, then back to `glyph` |
| `tone` | `light` (white arrow, dark outline), `dark`, or `outline` (transparent fill, white outline) |
| `color` / `outline_color` | Override `tone` |
| `click_ring` | `subtle` / `standard` / `bold` / `none` |
| `path` | Waypoints `{time, x, y, click?}` — the pointer clicks on arrival unless the waypoint says `"click": false` |
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

## `outline` tone: a pointer that doesn't fight the thing it's pointing at

`light` and `dark` are both filled arrows — a solid shape that sits on top of whatever's underneath. `outline` is a third tone: a transparent fill with a white outline, so the arrow reads as a mark rather than a shape competing for attention with the control it's pointing at. Reach for it over a busy screenshot or a mockup where a filled arrow would cover detail you want to keep visible.

The click ring follows the same rule as `color`/`outline_color`: it defaults to whichever colour is actually visible for the tone in use — the fill for `light`/`dark`, the outline for `outline` — rather than a colour hard-coded independently of `tone`. `ring_color` still overrides it directly, same as on the filled tones.

## `glyph` / `click_glyph`: a hand that closes on click

The default `glyph` is the classic arrow. `hand` draws an open hand pointing with its index finger, fingertip at the hotspot — the same point `path`/`click_at` coordinates always referred to. `grab` draws the same hand with the finger retracted into the fist, as if it had just closed around that point.

```json
{
  "type": "pointer",
  "glyph": "hand",
  "click_glyph": "grab",
  "tone": "outline",
  "path": [
    { "time": 0.0, "x": 1500, "y": 900 },
    { "time": 0.6, "x": 960, "y": 540 }
  ],
  "click_at": [0.7]
}
```

`click_glyph` only applies **for the duration of the click** — the same window `click_progress` already computes from `path`'s arrival times or `click_at`. There is no second clock: a hand with no `click_glyph` set still dips on click (the existing scale animation), it just never swaps shape. `grab` with no `glyph: "hand"` is legal but unusual — nothing ever shows it, since the resting glyph never becomes it outside a click.

Because the hotspot is defined per glyph rather than being the geometric centre of its bounding box, switching `glyph` (or swapping to `click_glyph` mid-click) never moves the point a waypoint is aimed at — only the drawn shape around that point changes.

## Travelling through a point without clicking

A waypoint clicks on arrival, which is what you want for the control the
walkthrough is about and wrong for the corner it rounds on the way there. Set
`"click": false` on the ones that are only a path:

```json
"path": [
  { "time": 0.0, "x": 120, "y": 620 },
  { "time": 0.6, "x": 500, "y": 620, "click": false },
  { "time": 1.2, "x": 500, "y": 240 }
]
```

The default is `true`, so a scenario written before this field existed is
unchanged. `click_at` stays ignored while a `path` is present — a pointer that
should click somewhere says so on the waypoint, not beside it.

## `tone: "outline"` reads on a light frame too

The unfilled tone is a white stroke over a transparent fill. On a white frame
that was invisible, although the tone exists precisely so the pointer can sit on
any background without a filled shape competing with what it points at. It now
carries a dark contour under the white one, so the glyph reads either way.

Setting `outline_color` yourself turns the contour off — you have said what the
edge should be, and a second edge under it would be a surprise.
