# Rule: `number_wheel` vs `counter`

Two components display an animated number. They don't tell the same story.

**`counter`** interpolates a *value* and rewrites the number every frame. It answers "how much, right now?" — a rising gauge, an accumulating total. Its glyphs jump, because 8,999 and then 9,000 have nothing in common.

**`number_wheel`** scrolls strips of digits, like a mechanical odometer. It answers "the figure lands" — a KPI settling, a result being revealed. What you watch is the motion; what remains is the requested digit.

```json
{
  "type": "number_wheel",
  "value": "30,222",
  "spin": "double",
  "duration": 1.1,
  "delay": 0.3,
  "stagger_per_column": 0.09,
  "style": { "font-size": 120, "font-weight": 700, "color": "#38BDF8" }
}
```

| Field | Role | Default |
|---|---|---|
| `value` | The figure exactly as written: `"30,222"`, `"5.7"`, `"98%"` | required |
| `spin` | `single` / `double` / `triple` — 0-9 loops before landing | `single` |
| `duration` | Landing time for **one** reel | `1.2` |
| `delay` | Before the first reel starts | `0` |
| `stagger_per_column` | Offset per column, left to right | `0.08` |
| `easing` | Curve of the travel | `ease_out_cubic` |

## `value` is a string, not a number

The digits roll; everything else — comma, dot, sign, unit — is painted where it stands, motionless. That's what lets you write `"1,204 €"` or `"98%"` without the separator going haywire.

## `spin` changes the speed, not the duration

Every reel takes `duration` no matter what. `triple` doesn't make the animation longer: it scrolls three times as many digits in the same time. A `triple` on a short `duration` turns into an unreadable blur.

## `stagger_per_column: 0` is a default to avoid

All the reels land together, which reads as a single flip. The left-to-right offset is what makes the last digit the one that *settles* the figure.

## The box reserves space for the widest digit

Each column is as wide as the widest digit in the font, not the width of the final digit — otherwise a `111` would reserve a narrow box and then overflow while a `0` scrolls past. The validator measures the same thing.
