# Rule: Compose, Don't Catalogue

Read this before reaching for a UI-widget component (`stat`, `badge`, `gauge`, `progress`, `stepper`, and 27 others — the full list is in `SKILL.md`'s "Composition over cataloguing"). They still exist and still render, but they are frozen arrangements of primitives, baked with one particular art direction. A generator's default reflex should be to **compose the shape from primitives**, not to fill in a pre-built one.

`terminal`, `codeblock`, and `notification` used to be three more entries in that frozen-widget class. They are not — they were deleted outright, not deprecated, so they don't belong in the lookup table below (a deleted type is not "instead of X, compose Y", it's just gone). Their recipes are kept — see "Recipe: terminal, code block, toast (deleted components)" below.

This is the same reflex as [rules/templates-and-iteration.md](templates-and-iteration.md) — read that file for the mechanics of `components`/`for-each`/`use` (bindings, `$index`, param defaults, pass ordering, named errors). This file is the *when* and *what-to-build*; that one is the *how*.

## The reflex

1. **Sketch the shape as HTML/CSS**, same as any other layout (see the "Mental Model" section at the top of `SKILL.md`): a KPI card is a `card` with an icon, a big number, and a small label stacked in a column. A pill is a rounded `div` with an icon and text in a row. A progress bar is a track behind a fill.
2. **If it appears once**, just write it as a normal `card`/`div`/`text`/`shape` tree.
3. **The moment it appears more than once** — three KPI cards, five feature pills, a row of progress bars — define it once under `components` and instantiate it with `for-each`. Ten hand-copied cards is the most common failure mode in generation (one of them always drifts on a color or a forgotten field); one template with a data array cannot drift.

## Lookup: frozen widget → primitive recipe

| Instead of | Compose from | Worked example |
|---|---|---|
| `stat` | `card` (background/radius/shadow) + `icon` + `text` (value, large) + `text` (label, small) | `examples/composition-kpi-row.json` |
| `badge` | `div` (rounded pill, colored background) + `icon` + `text` | `examples/composition-pill-row.json` |
| `progress` (linear) | a `card` track (fixed width, flat color) containing a `shape` (`rounded_rect`) fill whose `style.width` is keyframed from a small value to the target | `examples/composition-progress-bars.json` |
| `stepper` / `timeline` | `card` circles (node) + `text` (label) + `shape` (`rect`) connectors, one `for-each` item emitting the node and its trailing connector as sibling output (`template` as an array — see below) | `examples/composition-step-flow.json` |
| `avatar` / `avatar_group` | `shape` (`circle`, or an `image` with `fit: "cover"` clipped by a circular container) with `margin-left` negative overlap in a `flex-direction: row` container | — |
| `tooltip` / `callout` | `card` (small, rounded) + `shape` (`triangle`, rotated for the arrow) + `text` | — |
| `switch` / `slider` | two `shape`s (track + thumb), thumb position/track fill keyframed like the progress-bar recipe | — |
| `divider` | a single `shape` (`rect`), full width or full height, 1-2px thick | — |
| `list` / `rating` | `for-each` over items, each rendering an `icon` + `text` (or repeated star `icon`s with a partial-fill trick via two overlapping copies, one clipped) | — |
| `chart` | `for-each` over the data with a computed `height`/`width` expression per bar — proportional size and an index-staggered grow-in from one expression, e.g. `"height": "= $v * 3.4 * clamp(($t - 0.2 - $i*0.12) * 1.6, 0, 1)"` | — |
| `heatmap` | `for-each` over the cells with a computed `fill` expression per cell, driven by the cell's own value | — |
| `table` | `for-each` over the rows inside a `grid`-styled container — one row per grid row, cells as `text` children | — |
| `particle` | `for-each` over N items with `rand(seed, $i)` for placement and `sin($t)` for drift — deterministic by construction, not actually random | — |
| `caption` | `for-each` over the words with `start_at`/`end_at` per word and an expression picking the active one | — |
| `mockup` | a `shape` frame (device silhouette) around an `image` | — |

`gauge` (an arc) and `number_wheel` (rolling digit strips) are the two widest exceptions: reproducing them needs either `svg` path arcs with `draw_progress` or genuine per-digit scroll physics — mechanically harder than card/text/shape. Using the component directly for these two is reasonable; the point of this rule is the *default reflex*, not a ban.

## Recipe: KPI card (`stat` replacement)

```json
{
  "components": {
    "kpi_card": {
      "params": {
        "label": { "type": "string" },
        "value": { "type": "string" },
        "accent": { "type": "string", "default": "#6366F1" }
      },
      "template": {
        "type": "card",
        "style": { "width": 360, "height": 220, "background": "#111827", "border-radius": 20, "padding": 28, "flex-direction": "column", "justify-content": "space-between" },
        "children": [
          { "type": "text", "content": "$value", "style": { "font-size": 56, "color": "#FFFFFF", "font-weight": "bold" } },
          { "type": "text", "content": "$label", "style": { "font-size": 22, "color": "#94A3B8" } }
        ]
      }
    }
  },
  "children": [{
    "for-each": [
      { "label": "Active Users", "value": "45.2K", "accent": "#22C55E" },
      { "label": "Revenue", "value": "1.24M", "accent": "#3B82F6" }
    ],
    "template": { "use": "kpi_card", "props": { "label": "$label", "value": "$value", "accent": "$accent" } }
  }]
}
```

Full version with icon, accent shape, and staggered entrance: `examples/composition-kpi-row.json`.

## Recipe: animated progress bar (`progress` replacement)

A track and a fill are two `shape`s, not one component. The fill's `style.width` is a normal `keyframes` animation, exactly like animating any other property — there is nothing progress-bar-specific about it:

```json
{
  "type": "card",
  "style": { "width": 760, "height": 18, "background": "#1E293B", "border-radius": 9, "padding": 0 },
  "children": [{
    "type": "shape",
    "shape": "rounded_rect",
    "fill": "#3B82F6",
    "style": {
      "width": 6, "height": 18, "border-radius": 9,
      "animation": [{
        "name": "keyframes",
        "keyframes": [{ "property": "width", "easing": "ease_out_cubic",
          "keyframes": [{ "time": 0, "value": 6 }, { "time": 1.2, "value": 623 }] }]
      }]
    }
  }]
}
```

Start the fill's keyframe `width` a few pixels above zero, not at zero — a `rounded_rect` with `width: 0` still has to paint the radius on both ends and can pop rather than grow. Full version with label/percent rows and four bars via `for-each`: `examples/composition-progress-bars.json`.

## Recipe: `for-each` emitting siblings, not just one node

A `for-each template` can be an **array**, not just a single object — each element of the array is inserted as a sibling, not nested. This is what a step-flow needs: each data item produces both a step node *and* the connector that follows it, without a second pass to "join the dots":

```json
"template": [
  { "type": "card", "style": { "width": 88, "height": 88, "border-radius": 44 }, "children": [ ... ] },
  { "type": "shape", "shape": "rect", "fill": "$connector_color", "style": { "width": 140, "height": 4 } }
]
```

Give the **last** item's data row a transparent `connector_color` (`"#00000000"`) instead of trying to omit the trailing connector conditionally — `for-each` has no branching, so the cleanest way to special-case the last element is to make its data say so explicitly. Full version: `examples/composition-step-flow.json`.

## Recipe: terminal, code block, toast (deleted components)

`terminal`, `codeblock`, and `notification` are not deprecated like the widgets in the table above — they were removed from the engine entirely, files and all. Unlike a frozen widget, there is no fallback to "use the component directly for the one-off case": the type does not deserialize any more. These three recipes are what replaces them.

**Terminal** — a `div` title bar (three coloured `shape` circles + a `text` label) over a column of monospace `text` lines, one node per printed line so each can carry its own colour (a prompt line in green, output in grey, a highlighted result line in a different colour again — a single component could never do that per-line). A `typewriter` animation on each line, staggered via the column's own `stagger` field, reproduces the old line-by-line/typewriter reveal:

```json
{
  "type": "div",
  "style": { "flex-direction": "column", "background": "#1E1E1E", "border-radius": 10, "overflow": "hidden", "width": 900, "height": 300 },
  "children": [
    {
      "type": "div",
      "style": { "flex-direction": "row", "align-items": "center", "gap": 8, "padding": { "top": 10, "right": 14, "bottom": 10, "left": 14 }, "background": "#2D2D2D" },
      "children": [
        { "type": "shape", "shape": "circle", "fill": "#ff5f56", "style": { "width": 12, "height": 12 } },
        { "type": "shape", "shape": "circle", "fill": "#ffbd2e", "style": { "width": 12, "height": 12 } },
        { "type": "shape", "shape": "circle", "fill": "#27c93f", "style": { "width": 12, "height": 12 } },
        { "type": "text", "content": "rustmotion", "style": { "font-size": 13, "color": "#808080", "margin": { "left": 8 } } }
      ]
    },
    {
      "type": "div",
      "stagger": 0.4,
      "style": { "flex-direction": "column", "padding": 20, "gap": 6 },
      "children": [
        { "type": "text", "content": "$ rustmotion validate -f scene.json", "style": { "font-family": "JetBrains Mono", "font-size": 16, "color": "#22C55E", "white-space": "pre", "animation": [{ "name": "typewriter", "duration": 0.4 }] } },
        { "type": "text", "content": "schema:   pass", "style": { "font-family": "JetBrains Mono", "font-size": 16, "color": "#A0A0A0", "white-space": "pre", "animation": [{ "name": "typewriter", "duration": 0.4 }] }, "caret": { "shape": "block", "hide_when_done": true } }
      ]
    }
  ]
}
```

The old `TerminalTheme::Dark` palette (bg `#1E1E1E`, chrome bar `#2D2D2D`, prompt `#22C55E`, command `#FFFFFF`, output `#A0A0A0`, title `#808080`) is worth keeping verbatim — it's a calibrated three-colour system, not an arbitrary choice. `text.caret` on the last line reproduces the blinking reveal-head cursor.

**Code block** — one `rich_text` per source line, each with a hand-tokenized `spans` array (`{"text": "fn", "color": "#bb9af7"}`, one span per keyword/identifier/punctuation run — merge adjacent same-coloured tokens rather than emitting one span per character). This is the piece that used to be `syntect`-backed inside the engine; it moves to the generator because whoever is writing the JSON already knows the grammar of the language in the snippet — the tokenising never needed to be a runtime feature. Wrap the lines in the same title-bar `div` as the terminal recipe, and give each `rich_text` its own `typewriter` animation (staggered the same way) if the original had a `reveal`:

```json
{
  "type": "rich_text",
  "spans": [
    { "text": "fn ", "color": "#bb9af7" },
    { "text": "main", "color": "#7aa2f7" },
    { "text": "() {", "color": "#c0caf5" }
  ],
  "style": { "font-family": "JetBrains Mono", "font-size": 16, "white-space": "pre", "animation": [{ "name": "typewriter", "duration": 0.3 }] }
}
```

Beware a bare `=` ending up as its own span (an HTML tokenizer splitting `width="1920"` at the `=` boundary between two differently-coloured neighbours, for instance): a scenario string starting with `=` is read as an expression by the loader (`fold_value`'s `s.strip_prefix('=')`), and a solitary `"="` becomes an empty one — glue it onto a neighbouring span rather than emitting it standalone.

**Toast (`notification` replacement)** — a `div` card that gates its own visibility with `start_at`/`end_at` (the old `slide_in_at`/`slide_out_at` pair), with a `slide_in_*` entrance and a delayed `fade_out` exit so it doesn't just vanish at `end_at`:

```json
{
  "type": "div",
  "start_at": 1.5,
  "end_at": 4.8,
  "style": {
    "flex-direction": "row", "align-items": "center", "gap": 14,
    "background": "#111827", "border-radius": 14, "padding": 18, "width": 420,
    "animation": [
      { "name": "slide_in_left", "duration": 0.4 },
      { "name": "fade_out", "delay": 2.9, "duration": 0.35 }
    ]
  },
  "children": [
    { "type": "div", "style": { "width": 4, "align-self": "stretch", "background": "#10b981", "border-radius": 4 } },
    { "type": "icon", "icon": "lucide:check-circle", "style": { "width": 28, "height": 28, "color": "#10b981" } },
    { "type": "div", "style": { "flex-direction": "column", "gap": 4 }, "children": [
      { "type": "text", "content": "Build succeeded", "style": { "font-size": 20, "font-weight": "bold", "color": "#f8fafc" } },
      { "type": "text", "content": "All tests passed", "style": { "font-size": 15, "color": "#94a3b8" } }
    ] }
  ]
}
```

The left accent strip (a 4px-wide `div`, `align-self: stretch`) stands in for a border-and-variant-colour system without needing `style.border` at all. `fade_out`'s `delay` should land comfortably before `end_at - start_at` (here 3.3s of visible window), or the node disappears via the timing gate before the exit animation finishes playing.

All three recipes are exercised end-to-end in `examples/component-showcase.json`, `examples/mega-showcase.json`, `examples/rustmotion-promo.json`, and `examples/ferriskey-launch-60s.json` — each used to hold a `terminal`/`codeblock`/`notification` node and was rewritten with the compositions above.

## Pitfall: a literal `$` in for-each data

Variable substitution scans for `$name` tokens everywhere, including inside the *values* a `for-each` element supplies — not just inside the template. A KPI value like `"$1.24M"` gets read as an attempted reference to a variable named `1` and is left as literal text with a validator warning. Either escape it (`"$$1.24M"` → renders as `$1.24M`) or, simpler, keep the currency symbol out of the animated value and put it in a static label instead (`"Revenue (USD)"` / `"1.24M"`) — that's what `examples/composition-kpi-row.json` does.

## What this buys over the frozen component

- **Art direction is per video.** The frozen `stat` component has one accent-icon-in-a-circle look. A `card`+`icon`+`shape` recipe can match whatever palette and corner-radius language the rest of the video uses, because it's the same primitives as everything else in the scene — not a separately-styled widget bolted on.
- **It composes with everything else primitives already do** — `for-each` staggered entrances, `world` view camera framing, `components` nesting one recipe inside another (a KPI-card `components` entry can itself be used inside a dashboard-grid `components` entry).
- **Nothing is lost.** The frozen widgets keep working for the cases that genuinely warrant them (a one-off `gauge`, a `number_wheel` landing on a hero figure). This rule changes the default reflex, not the available vocabulary.
