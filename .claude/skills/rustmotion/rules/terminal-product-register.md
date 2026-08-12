# Rule: The Terminal-Product Register

A visual register for demonstrating a **command-line product**: two alternating
grounds, one statement per beat, a terminal window as the recurring subject, and
a mark that assembles from its own pixels at the end.

Every number below was measured off a 3840×2160 reference piece rather than
chosen, and the derivations are kept so you can re-derive them for a different
frame size. Values are given as **fractions of the frame width** — that is the
unit that survives a change of resolution; absolute pixel values are shown for
1920×1080 only as a convenience.

Use this register when the brief is "show what our CLI does". Do not use it for
data-heavy decks (see [chart-types.md](chart-types.md)) or for the statement-only
dark-premium look (see [1600-brutalist-style.md](1600-brutalist-style.md)).

---

## 1. Palette — five colours, no sixth

| Role | Hex | Where |
| --- | --- | --- |
| Warm ground | `#EEE9E2` | opening beat |
| Cold ground | `#F9F9F7` | statement cards |
| Accent | `#FB652A` | demo beats, the second colour in a headline |
| Terminal | `#101010` | the window's fill |
| Outro | `#0E0F1D` | the closing beat |
| Ink | `#1A1A1A` | all type on a light ground |

The accent is the **only** saturated colour. It carries the demo grounds, the
second line of every headline, and the mark. A second saturated hue anywhere
breaks the register — if the subject's brand has one, replace the accent
wholesale rather than adding to it.

---

## 2. Grounds — a flat colour plus one texture layer

A scene `background` is **either** a colour **or** presets, never both. The flat
ground is therefore a two-stop gradient of a single colour, with the texture
layered on top:

```json
"background": [
  { "preset": "gradient_shift", "speed": 0,
    "gradient_shift": { "colors": ["#FB652A", "#FB652A"], "gradient_type": "linear" } },
  { "preset": "pixel_grid", "speed": 1.0,
    "pixel_grid": { "colors": ["#FFFFFF33"], "size": 7, "spacing": 14,
                    "density": 0.32, "density_ramp": "edges",
                    "radius": 1, "motion": "twinkle" } }
]
```

**The demo ground's texture is a vignette, not a uniform scatter.** Measured
across the tenths of the frame, cell density runs

```
10.9 · 6.5 · 0.8 · 0.2 · 0.2 · 0.2 · 0.2 · 0.9 · 7.8 · 8.8 %
```

— heavy at both edges, effectively empty through the middle 60 %. That is what
keeps the texture off the window that sits in the centre. `density_ramp: "edges"`
does it; `radial` is its exact inverse and will crowd the subject.

**The light grounds carry a thin line grid**, which no preset draws directly.
Get it by negative: a full-density lattice whose cells are 2 px smaller than the
pitch, in a shade *lighter* than the ground, so the gaps read as lines.

```json
{ "preset": "pixel_grid", "speed": 1.0,
  "pixel_grid": { "colors": ["#FFFFFF"], "size": 428, "spacing": 430, "density": 1.0 } }
```

---

## 3. Typography — two scales, and one weight you must measure

**Two distinct scales, not one scaled by role.**

| Role | Fraction of width | 1920 | Use |
| --- | --- | --- | --- |
| Opening line | 0.036 | 69 px | one line across the frame, over the tiles |
| Statement display | 0.0747 | 143 px | the two-line cards |
| Body / caption | 0.015 | 29 px | the outro caption |

The statement block spans **66 % of the frame width** and sits **11.4 % in from
the left**. Calibrate the size against the block width, not the other way round:
change the font and the same `font-size` gives a different span.

**Colour changes per line, not inside one.** The first line takes the accent, the
second the ink:

```
One shell command,     ← accent
any server type        ← ink
```

**`line-height: 0.9`** — display type set tight enough that the two lines read as
one shape. **`letter-spacing: -2`.**

### Weight: measure the stem, not the ink

Ink coverage inside the block's bounding box is a **misleading** metric: it mixes
weight and proportions, and a narrower face shows less ink at the same weight. It
sent this reconstruction to `300` when the reference was a medium.

Measure the **stem width relative to the cap height** instead — that isolates
weight:

| | stem / cap |
| --- | --- |
| reference | 11.0 % |
| `font-weight: 300` | 7.8 % |
| **`font-weight: 400`** | **11.2 %** |
| `font-weight: 500 / 600 / 700` | 15.8 % |

The plateau above 400 is the tell: the default family ships three faces, so
**400 is its medium.** Asking for 500 gets bold.

Entrances are `char_fade_in` with `granularity: "word"`, `stagger: 0.05`,
`duration: 0.45`, and 0.18 s between the two lines. Word-by-word is what keeps a
3-second card from reading as a slide.

---

## 4. The terminal window — composed, not the `terminal` component

**`terminal` cannot express this.** `TerminalLine` carries one `color` for the
whole line, and this register colours *fragments inside* a line — a product name
in the accent inside a grey version string, a flag in cyan inside a sentence.
Build the window from parts and use `rich_text`, whose spans do carry per-span
colour.

### Geometry, measured

| | fraction | 1920×1080 |
| --- | --- | --- |
| width | 0.78 of frame width | 1498 px |
| left inset | 0.111 | 213 px |
| top | 0.12 of frame height | 130 px |
| bottom | lands **on** the frame's bottom edge | 1080 px |

The pane is anchored, not centred, and it bleeds to the bottom. Same 11 % margin
as the type — the register has one margin, used everywhere.

### Inside the pane

- **The transcript flows from the top.** Only the mode rule and the status bar
  are pinned to the bottom, by a `flex-grow: 1` spacer *between* content and
  rule. Pushing the whole body down with `justify-content: flex-end` empties the
  top and hangs the text at the bottom — wrong.
- Mono face: **`Menlo`**. `SF Mono` is not exposed under that name by the font
  manager and falls back to a proportional face **without a word**, which
  destroys the mono grid silently.
- `rich_text` span fields are **kebab-case** (`font-family`, `font-size`) and
  unknown keys are accepted and ignored. `font_family` in snake_case silently
  leaves every span at the component default.
- Set the size and family on the **component style too**, not only per span: the
  row's height comes from the component.

### The re-centring move

The pane enters large and bleeding, then un-zooms to the middle as the second
line of output appears:

```json
{"name": "keyframes", "delay": 1.16, "keyframes": [
  {"property": "scale",       "keyframes": [{"time": 0, "value": 1.0}, {"time": 0.55, "value": 0.78}], "easing": "ease_out"},
  {"property": "translate_y", "keyframes": [{"time": 0, "value": 0},   {"time": 0.55, "value": -65}],  "easing": "ease_out"}
]}
```

**`scale` alone does not re-centre.** Anchored top-left and bleeding past the
bottom, the pane's centre sits below the frame's; scaling in place leaves it
hanging low. The translate is what re-centres, and its value comes from the
geometry: `frame_centre_y − pane_centre_y`.

### One session, one pane

When two transcripts follow each other, keep **one** scene and hand over
*inside* the window — a scene change cross-fades the window itself, which a
continuous session never does. Each line carries its own schedule (`at` to
appear, `until` to leave), and the two transcripts are **overlaid**, not stacked:
a faded-out line keeps its box in the flex flow, so appending the second after
the first pushes it down by however many invisible rows the first had.

---

## 5. Rhythm — the demo beat dominates

The reference gives **41 %** of its runtime to the single session beat. Compress
by keeping that shape, not by trimming every beat evenly: a statement card reads
in 2.4 s, a terminal beat does not.

| beat | share |
| --- | --- |
| opening claim | 15 % |
| statement | 12 % |
| **session** | **41 %** |
| turn | 10 % |
| second command | 10 % |
| outro | 11 % |

### The duration arithmetic that bites

**Every transition overlaps the two scenes it joins**, so the scene durations
must sum to *more* than the target:

```
sum(durations) − sum(transition durations) = rendered duration
```

Writing 20 s of scenes with five 0.5 s transitions renders 17.5 s. Recompute this
whenever a transition's duration changes — it is the single most common way a
piece silently drifts off its target length.

---

## 6. Transitions — one gesture, used once

Default to `dissolve` at 0.5 s. Spend the one distinctive transition on the
**exit of the main demo**, where the piece turns:

```json
{ "type": "pixel_dissolve", "duration": 0.85, "cell": 46, "seed": 11 }
```

The frame turns over cell by cell, each cell fading on its own schedule, from the
**border inward** (`edges_in`, the default) so the subject in the centre is the
last thing to go. Mid-transition both scenes are on screen as a mosaic — that is
what separates it from `dissolve` (one global opacity, no structure) and from the
wipes (a single hard boundary).

Using it on every cut spends it. One occurrence reads as an intention; five read
as a filter.

`corner_reveal` is the same family — a rectangle anchored at a corner, uncovering
a still scene — and works where a harder, faster turn is wanted.

---

## 7. The outro — the mark builds from its own pixels

Cells appear one at a time over ~2.3 s, **bottom-up and centre-out**: the base
lands first, the arms close last. Each cell arrives **rotated** and settles
square, fading in rather than popping:

```json
"animation": [
  {"name": "fade_in", "delay": 0.10, "duration": 0.28},
  {"name": "keyframes", "delay": 0.10, "keyframes": [
    {"property": "rotation", "keyframes": [{"time": 0, "value": -14}, {"time": 0.42, "value": 0}], "easing": "ease_out"},
    {"property": "scale",    "keyframes": [{"time": 0, "value": 0.55}, {"time": 0.42, "value": 1}], "easing": "ease_out"}
  ]}
]
```

Use a **fixed tilt table**, not a random one: two renders of one scenario must be
identical, and a seeded RNG in the generator is one more thing to reproduce.

Stagger 0.075 s per cell. Colour the cells by **row**, in a top-to-bottom ramp —
the band structure is what makes a pixel mark read as a mark rather than as
confetti.

---

## 8. Applying this to another subject

What is **register** (keep):

- two alternating grounds, one accent, ink on light
- the two type scales and the per-line colour switch
- the anchored, bleeding pane and its re-centring move
- the demo beat taking ~40 % of the runtime
- one distinctive transition, at the turn
- a mark assembling from its own cells

What is **subject** (replace):

- the accent hex, and the outro's dark ground if the brand has one
- the tiles in the opening beat — they name the ecosystem the product plugs into
- every string in the transcript, and the mark's cell layout
- the mono content: keep it plausible and short enough that a line never wraps

What to **re-measure** for a new frame size: nothing, if you keep the fractions.
Everything, if you take the 1920 pixel values literally.

---

## Traps, collected

| Symptom | Cause |
| --- | --- |
| Type looks bold at any weight | Asking for 500; the family's medium is 400 |
| Mono text is proportional | Family `SF Mono` — not exposed; use `Menlo` |
| Every span at default size/font | Span fields are kebab-case; snake_case is ignored silently |
| Second transcript starts low | Faded-out lines keep their box; overlay instead of stacking |
| Pane shrinks but hangs low | `scale` without the compensating translate |
| Texture crowds the subject | `density_ramp: "radial"` instead of `"edges"` |
| Video is shorter than intended | Transition overlap not added back to the scene sum |
| Background rejected | A scene background is a colour **or** presets, never both |
| Left inset ignored on a scene | `Scene.layout.padding` is a single uniform value; use a container |
