# rustmotion

A CLI tool that renders motion design videos from JSON scenarios. No browser, no Node.js — just a single Rust binary.

[![Crates.io](https://img.shields.io/crates/v/rustmotion.svg)](https://crates.io/crates/rustmotion)
[![docs.rs](https://docs.rs/rustmotion/badge.svg)](https://docs.rs/rustmotion)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

## Install

```bash
cargo install rustmotion
```

**Requirements:** Rust toolchain + C++ compiler (for openh264). Optional: `ffmpeg` CLI for H.265/VP9/ProRes/WebM/GIF output.

## Quick Start

```bash
# Render a video
rustmotion render scenario.json -o video.mp4

# Render with a specific codec
rustmotion render scenario.json -o video.webm --codec vp9 --crf 30

# Export as PNG sequence
rustmotion render scenario.json -o frames/ --format png-seq

# Export as animated GIF
rustmotion render scenario.json -o output.gif --format gif

# Render a single frame for preview
rustmotion render scenario.json --frame 42 -o frame.png

# Validate without rendering
rustmotion validate scenario.json

# Export JSON Schema (for editor autocompletion or LLM prompts)
rustmotion schema -o schema.json

# Show scenario info
rustmotion info scenario.json
```

## CLI Reference

### `rustmotion render`

| Flag | Description | Default |
|---|---|---|
| `input` | Path to the JSON scenario file | (required) |
| `-o, --output` | Output file path | `output.mp4` |
| `--frame <N>` | Render a single frame to PNG (0-indexed) | |
| `--codec <CODEC>` | Video codec: `h264`, `h265`, `vp9`, `prores` | `h264` |
| `--crf <0-51>` | Constant Rate Factor (lower = better quality) | `23` |
| `--format <FMT>` | Output format: `mp4`, `webm`, `mov`, `gif`, `png-seq` | auto from extension |
| `--transparent` | Transparent background (PNG sequence, WebM, ProRes 4444) | `false` |
| `--output-format json` | Machine-readable JSON output for CI pipelines | |
| `-q, --quiet` | Suppress all output except errors | |

---

## JSON Scenario Format

```json
{
  "version": "1.0",
  "video": { ... },
  "audio": [ ... ],
  "scenes": [ ... ]
}
```

### Video Config

```json
{
  "video": {
    "width": 1080,
    "height": 1920,
    "fps": 30,
    "background": "#0f172a",
    "codec": "h264",
    "crf": 23
  }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `width` | `u32` | (required) | Video width in pixels (must be even) |
| `height` | `u32` | (required) | Video height in pixels (must be even) |
| `fps` | `u32` | `30` | Frames per second |
| `background` | `string` | `"#000000"` | Default background color (hex) |
| `codec` | `string` | `"h264"` | Video codec: `h264`, `h265`, `vp9`, `prores` |
| `crf` | `u8` | `23` | Constant Rate Factor (0-51, lower = better quality) |

### Audio Tracks

```json
{
  "audio": [
    {
      "src": "music.mp3",
      "start": 0.0,
      "end": 10.0,
      "volume": 0.8,
      "fade_in": 1.0,
      "fade_out": 2.0
    }
  ]
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `src` | `string` | (required) | Path to audio file (MP3, WAV, OGG, FLAC, AAC) |
| `start` | `f64` | `0.0` | Start time in the output video (seconds) |
| `end` | `f64` | | End time (omit for full track) |
| `volume` | `f32` | `1.0` | Volume multiplier (0.0 - 1.0) |
| `fade_in` | `f64` | | Fade-in duration (seconds) |
| `fade_out` | `f64` | | Fade-out duration (seconds) |

---

## Scenes

Each scene is an **implicit flex container** at video dimensions (default direction: `column`). Children participate in flex flow automatically. Use `positioned` container for absolute positioning.

```json
{
  "scenes": [
    {
      "duration": 3.0,
      "background": "#1a1a2e",
      "layout": {
        "direction": "column",
        "align_items": "center",
        "justify_content": "center",
        "gap": 24
      },
      "children": [ ... ],
      "transition": {
        "type": "fade",
        "duration": 0.5
      }
    }
  ]
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `duration` | `f64` | (required) | Scene duration in seconds |
| `background` | `string` | | Scene background (overrides `video.background`) |
| `freeze_at` | `f64` | | Freeze the scene at this time (seconds) |
| `children` | `Component[]` | `[]` | Components rendered bottom-to-top |
| `layout` | `object` | | Scene-level flex layout options |
| `transition` | `Transition` | | Transition effect from the previous scene |

**`layout` options:** `direction` (column/row), `gap`, `align_items` (start/center/end/stretch), `justify_content` (start/center/end/space_between/space_around/space_evenly), `padding`

---

## Transitions

Transitions blend between two consecutive scenes. Set on the **second** scene.

```json
{
  "transition": {
    "type": "clock_wipe",
    "duration": 0.8
  }
}
```

| Type | Description |
|---|---|
| `fade` | Linear crossfade between scenes |
| `wipe_left` | Horizontal wipe revealing scene B from the left |
| `wipe_right` | Horizontal wipe revealing scene B from the right |
| `wipe_up` | Vertical wipe revealing scene B from the top |
| `wipe_down` | Vertical wipe revealing scene B from the bottom |
| `zoom_in` | Scene A zooms in and fades out, revealing scene B |
| `zoom_out` | Scene B zooms out from larger to normal size |
| `flip` | 3D Y-axis flip simulation |
| `clock_wipe` | Circular clockwise sweep from 12 o'clock |
| `iris` | Expanding circle from the center reveals scene B |
| `slide` | Scene B pushes scene A to the left |
| `dissolve` | Per-pixel noise dissolve |
| `none` | Hard cut at the midpoint |

| Field | Type | Default | Description |
|---|---|---|---|
| `type` | `string` | (required) | One of the transition types above |
| `duration` | `f64` | `0.5` | Transition duration in seconds |

---

## Include (Composable Scenarios)

Scene entries can reference external scenario files to inject their scenes inline:

```json
{
  "scenes": [
    { "include": "shared/intro.json" },
    { "duration": 5.0, "children": [ ... ] },
    { "include": "shared/outro.json", "scenes": [0, 2] }
  ]
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `include` | `string` | (required) | Path (relative to parent file) or URL to a scenario JSON |
| `scenes` | `usize[]` | | Only include scenes at these 0-based indices. Omit to include all |

- The included file's `video` config is ignored
- Audio tracks from included files are merged
- Includes can be nested (max depth: 8)

---

## Components

All components are discriminated by `"type"`. Rendered in array order (first = bottom, last = top).

### Common Fields

Available on all component types:

| Field | Type | Default | Description |
|---|---|---|---|
| `start_at` | `f64` | | Component appears at this time (seconds within scene) |
| `end_at` | `f64` | | Component disappears after this time |

### Common Style Fields

All visual properties are inside a `"style"` object:

| Style field | Type | Default | Description |
|---|---|---|---|
| `opacity` | `f32` | `1.0` | 0.0 to 1.0 |
| `padding` | `f32 \| {top, right, bottom, left}` | | Inner spacing |
| `margin` | `f32 \| {top, right, bottom, left}` | | Outer spacing |
| `animation` | `array \| object` | `[]` | Animation effects (see [Animations](#animations)) |

---

### Text

```json
{
  "type": "text",
  "content": "Hello World",
  "max_width": 800,
  "style": {
    "font-size": 48,
    "color": "#FFFFFF",
    "font-family": "Inter",
    "font-weight": "bold",
    "text-align": "center",
    "line-height": 1.2,
    "letter-spacing": 2.0,
    "animation": [{ "name": "fade_in_up", "delay": 0.3, "duration": 0.6 }]
  }
}
```

**Root fields:** `content` (required), `max_width`

| Style field | Type | Default | Description |
|---|---|---|---|
| `font-size` | `f32` | `48.0` | Font size in pixels |
| `color` | `string` | `"#FFFFFF"` | Text color (hex) |
| `font-family` | `string` | `"Inter"` | Font family name |
| `font-weight` | `enum` | `"normal"` | `"normal"` or `"bold"` |
| `font-style` | `enum` | `"normal"` | `"normal"`, `"italic"`, `"oblique"` |
| `text-align` | `enum` | `"left"` | `"left"`, `"center"`, `"right"` |
| `line-height` | `f32` | | Line height multiplier |
| `letter-spacing` | `f32` | | Additional spacing between characters |
| `text-shadow` | `object` | | `{ "color": "#000", "offset_x": 2, "offset_y": 2, "blur": 4 }` |
| `stroke` | `object` | | `{ "color": "#000", "width": 2 }` |
| `text-background` | `object` | | `{ "color": "#000", "padding": 4, "corner_radius": 4 }` |

---

### Shape

```json
{
  "type": "shape",
  "shape": "rounded_rect",
  "size": { "width": 300, "height": 200 },
  "style": {
    "fill": "#3b82f6",
    "border-radius": 16,
    "stroke": { "color": "#ffffff", "width": 2 },
    "animation": [{ "name": "scale_in", "duration": 0.6 }]
  }
}
```

**Root fields:** `shape` (required), `size`, `text`

| Style field | Type | Default | Description |
|---|---|---|---|
| `fill` | `string \| gradient` | | Fill color (hex) or gradient object |
| `stroke` | `{color, width}` | | Stroke outline |
| `border-radius` | `f32` | | Corner radius (for `rounded_rect`) |

**Shape types:** `rect`, `circle`, `rounded_rect`, `ellipse`, `triangle`, `star` (with `points`, default 5), `polygon` (with `sides`, default 6), `path` (with `data` SVG path string)

**Gradient fill:**
```json
{
  "fill": {
    "type": "linear",
    "colors": ["#667eea", "#764ba2"],
    "angle": 135,
    "stops": [0.0, 1.0]
  }
}
```

Types: `linear`, `radial`.

**Embedded text in shapes (`text` field):**
```json
{
  "type": "shape",
  "shape": "circle",
  "size": { "width": 56, "height": 56 },
  "style": { "fill": "#2A74FF" },
  "text": {
    "content": "1",
    "font_size": 22,
    "color": "#FFFFFF",
    "font_weight": "bold",
    "align": "center",
    "vertical_align": "middle"
  }
}
```

`vertical_align`: `"top"`, `"middle"`, `"bottom"` (default: `"middle"`).

---

### Image

```json
{
  "type": "image",
  "src": "photo.png",
  "size": { "width": 1080, "height": 1080 },
  "fit": "cover",
  "style": {
    "animation": [{ "name": "fade_in", "duration": 0.5 }]
  }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `src` | `string` | (required) | Path to image file (PNG, JPEG, WebP) |
| `size` | `{width, height}` | | Target size (uses native image size if omitted) |
| `fit` | `string` | `"cover"` | `"cover"`, `"contain"`, `"fill"`, `"none"` |

---

### SVG

```json
{
  "type": "svg",
  "src": "logo.svg",
  "size": { "width": 200, "height": 200 }
}
```

Or with inline SVG:

```json
{
  "type": "svg",
  "data": "<svg viewBox='0 0 100 100'><circle cx='50' cy='50' r='40' fill='red'/></svg>"
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `src` | `string` | | Path to `.svg` file |
| `data` | `string` | | Inline SVG markup |
| `size` | `{width, height}` | | Target size (uses SVG intrinsic size if omitted) |

One of `src` or `data` is required.

---

### Icon

Renders an icon from the [Iconify](https://iconify.design/) open-source framework (200,000+ icons from 150+ sets). Icons are fetched from the Iconify API at render time.

Browse all available icons at [icon-sets.iconify.design](https://icon-sets.iconify.design/).

```json
{
  "type": "icon",
  "icon": "lucide:home",
  "size": { "width": 64, "height": 64 },
  "style": { "color": "#38bdf8" }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `icon` | `string` | (required) | Iconify identifier `"prefix:name"` (e.g. `"lucide:home"`, `"mdi:account"`) |
| `size` | `{width, height}` | `24x24` | Icon size in pixels |

Style: `color` (default `"#FFFFFF"`)

**Common icon sets:**

| Prefix | Name | Best for |
|---|---|---|
| `lucide` | Lucide | Clean UI icons (default choice) |
| `mdi` | Material Design Icons | Material UI, Android |
| `heroicons` | Heroicons | Tailwind projects |
| `ph` | Phosphor | Modern UI |
| `tabler` | Tabler Icons | Dashboards |
| `simple-icons` | Simple Icons | Brand/company logos |
| `devicon` | Devicon | Programming language logos |

---

### Video

Embeds a video clip as a component. Requires `ffmpeg` on PATH.

```json
{
  "type": "video",
  "src": "clip.mp4",
  "size": { "width": 1080, "height": 1920 },
  "trim_start": 2.0,
  "trim_end": 8.0,
  "playback_rate": 0.5,
  "fit": "cover",
  "volume": 0.0
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `src` | `string` | (required) | Path to video file |
| `size` | `{width, height}` | (required) | Display size |
| `trim_start` | `f64` | `0.0` | Start offset in the source clip (seconds) |
| `trim_end` | `f64` | | End offset in the source clip (seconds) |
| `playback_rate` | `f64` | `1.0` | Playback speed (0.5 = half speed, 2.0 = double) |
| `fit` | `string` | `"cover"` | `"cover"`, `"contain"`, `"fill"` |
| `volume` | `f32` | `1.0` | Audio volume (0.0 = mute) |
| `loop_video` | `bool` | | Loop the clip |

---

### GIF

Displays an animated GIF, synced to the scene timeline.

```json
{
  "type": "gif",
  "src": "animation.gif",
  "size": { "width": 300, "height": 300 },
  "fit": "cover"
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `src` | `string` | (required) | Path to `.gif` file |
| `size` | `{width, height}` | | Display size (uses GIF native size if omitted) |
| `fit` | `string` | `"cover"` | `"cover"`, `"contain"`, `"fill"` |
| `loop_gif` | `bool` | `true` | Loop the GIF animation |

---

### Caption

Timed word-by-word captions with active word highlighting.

```json
{
  "type": "caption",
  "words": [
    { "text": "Hello", "start": 0.0, "end": 0.5 },
    { "text": "world!", "start": 0.5, "end": 1.0 }
  ],
  "mode": "highlight",
  "active_color": "#FFD700",
  "max_width": 900,
  "style": { "font-size": 48, "color": "#FFFFFF" }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `words` | `array` | (required) | `[{ "text", "start", "end" }]` |
| `mode` | `enum` | `"default"` | `"default"`, `"highlight"`, `"karaoke"`, `"bounce"` |
| `active_color` | `string` | `"#FFD700"` | Active word color |
| `max_width` | `f32` | | Maximum width before word-wrapping |

Style: `font-size` (48.0), `font-family`, `color` (#FFFFFF)

---

### Counter

Animated number counter. Must be used standalone (not inside a card).

```json
{
  "type": "counter",
  "from": 0,
  "to": 1250,
  "decimals": 0,
  "separator": " ",
  "suffix": "€",
  "easing": "ease_out",
  "start_at": 0.5,
  "end_at": 2.5,
  "style": {
    "font-size": 72,
    "color": "#FFFFFF",
    "font-weight": "bold",
    "text-align": "center"
  }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `from` | `f64` | (required) | Start value |
| `to` | `f64` | (required) | End value |
| `decimals` | `u8` | `0` | Number of decimal places |
| `separator` | `string` | | Thousands separator (e.g. `" "`, `","`) |
| `prefix` | `string` | | Text before the number (e.g. `"$"`) |
| `suffix` | `string` | | Text after the number (e.g. `"%"`, `"€"`) |
| `easing` | `string` | `"linear"` | Easing for the counter interpolation |

Style: `font-size`, `color`, `font-family`, `font-weight`, `text-align`, `letter-spacing`, `text-shadow`, `stroke`

---

### Positioned

Container that places children at fixed absolute coordinates (like Flutter's `Stack`/`Positioned`). Each child uses `position: {x, y}` relative to the container's top-left.

```json
{
  "type": "positioned",
  "children": [
    { "type": "shape", "shape": "rect", "position": { "x": 0, "y": 0 }, "size": { "width": 400, "height": 300 }, "style": { "fill": "#1E293B", "border-radius": 16 } },
    { "type": "icon", "icon": "lucide:phone-off", "position": { "x": 170, "y": 120 }, "size": { "width": 64, "height": 64 }, "style": { "color": "#FFFFFF" } }
  ]
}
```

---

### Card / Flex

Visual container with CSS-like flex & grid layout. `flex` is an alias for `card`. Each dimension of `size` can be a number or `"auto"`.

**Flex example:**
```json
{
  "type": "card",
  "size": { "width": 800, "height": "auto" },
  "style": {
    "flex-direction": "row",
    "align-items": "center",
    "gap": 16,
    "padding": 24,
    "background": "#1E293B",
    "border-radius": 16,
    "animation": [{ "name": "fade_in_up", "delay": 0.3, "duration": 0.6 }]
  },
  "children": [
    { "type": "icon", "icon": "lucide:check-circle", "size": { "width": 48, "height": 48 }, "style": { "color": "#22C55E" } },
    { "type": "text", "content": "Feature enabled", "style": { "font-size": 32, "color": "#FFFFFF" } }
  ]
}
```

**Grid example (2x2):** Grid containers need an explicit `height` (not `"auto"`) to prevent rows from stretching.
```json
{
  "type": "card",
  "size": { "width": 600, "height": 400 },
  "style": {
    "display": "grid",
    "grid-template-columns": [{ "fr": 1 }, { "fr": 1 }],
    "grid-template-rows": [{ "fr": 1 }, { "fr": 1 }],
    "gap": 16,
    "padding": 24,
    "background": "#1a1a2e"
  },
  "children": [
    { "type": "text", "content": "Cell 1", "style": { "color": "#FFFFFF" } },
    { "type": "text", "content": "Cell 2", "style": { "color": "#FFFFFF" } },
    { "type": "text", "content": "Cell 3", "style": { "color": "#FFFFFF" } },
    { "type": "text", "content": "Cell 4", "style": { "color": "#FFFFFF" } }
  ]
}
```

**Style fields:**

| Style field | Type | Default | Description |
|---|---|---|---|
| `display` | `enum` | `"flex"` | `"flex"` or `"grid"` |
| `background` | `string` | | Background color (hex) |
| `border-radius` | `f32` | `12.0` | Corner radius |
| `border` | `object` | | `{ "color": "#E5E7EB", "width": 1 }` |
| `box-shadow` | `object` | | `{ "color": "#00000040", "offset_x": 0, "offset_y": 4, "blur": 12 }` |
| `padding` | `f32 \| object` | | Inner spacing |
| `flex-direction` | `enum` | `"column"` | `"column"`, `"row"`, `"column_reverse"`, `"row_reverse"` |
| `flex-wrap` | `bool` | `false` | Wrap children to next line |
| `align-items` | `enum` | `"start"` | `"start"`, `"center"`, `"end"`, `"stretch"` |
| `justify-content` | `enum` | `"start"` | `"start"`, `"center"`, `"end"`, `"space_between"`, `"space_around"`, `"space_evenly"` |
| `gap` | `f32` | `0` | Spacing between children |
| `grid-template-columns` | `array` | | `[{"px": N}, {"fr": N}, "auto"]` |
| `grid-template-rows` | `array` | | Same format as columns |

**Per-child layout properties** (in child `"style"`):
- `flex-grow` (f32) — default 0
- `flex-shrink` (f32) — default 1
- `flex-basis` (f32) — defaults to natural size
- `align-self` (enum) — `"start"`, `"center"`, `"end"`, `"stretch"`
- `grid-column` (object) — `{ "start": 1, "span": 2 }` (1-indexed)
- `grid-row` (object) — `{ "start": 1, "span": 2 }` (1-indexed)

---

### Codeblock

Code block with syntax highlighting, chrome, reveal animations, and animated diff transitions.

```json
{
  "type": "codeblock",
  "code": "fn main() {\n    println!(\"Hello\");\n}",
  "language": "rust",
  "theme": "base16-ocean.dark",
  "show_line_numbers": true,
  "chrome": { "enabled": true, "title": "main.rs" },
  "reveal": { "mode": "typewriter", "start": 0, "duration": 2.5 },
  "style": { "font-size": 18, "border-radius": 12, "padding": 16 },
  "states": [
    {
      "code": "fn main() {\n    println!(\"Hello, world!\");\n}",
      "at": 5.0,
      "duration": 2.0,
      "cursor": { "enabled": true, "blink": true }
    }
  ]
}
```

**Root fields:** `code` (required), `language`, `theme`, `size`, `show_line_numbers`, `chrome`, `highlights`, `reveal`, `states`

| Style field | Type | Default |
|---|---|---|
| `font-family` | `string` | `"JetBrains Mono"` |
| `font-size` | `f32` | `14.0` |
| `font-weight` | `enum` | `"normal"` |
| `line-height` | `f32` | `1.5` (multiplier) |
| `background` | `string` | (uses theme) |
| `border-radius` | `f32` | `12.0` |
| `padding` | `f32 \| object` | `16` |

#### Chrome (Title Bar)

| Field | Type | Default | Description |
|---|---|---|---|
| `chrome.enabled` | `bool` | `true` | Show the title bar |
| `chrome.title` | `string` | | Title text (e.g. filename) |

#### Line Highlights

```json
{ "highlights": [{ "lines": [2], "color": "#FFFF0022", "start": 3.0, "end": 4.5 }] }
```

#### Reveal Animation

| Field | Type | Default | Description |
|---|---|---|---|
| `reveal.mode` | `string` | (required) | `"typewriter"` or `"line_by_line"` |
| `reveal.start` | `f64` | `0.0` | Start time (seconds) |
| `reveal.duration` | `f64` | `1.0` | Duration (seconds) |

#### Code States (Diff Transitions)

Animate between code versions with automatic diff detection.

| Field | Type | Default | Description |
|---|---|---|---|
| `states[].code` | `string` | (required) | New code content |
| `states[].at` | `f64` | (required) | Transition start time |
| `states[].duration` | `f64` | `0.6` | Transition duration |
| `states[].cursor.enabled` | `bool` | `true` | Show editing cursor |
| `states[].cursor.blink` | `bool` | `true` | Blink the cursor |

#### Available Themes (72)

**Syntect built-in:** `base16-ocean.dark`, `base16-ocean.light`, `base16-eighties.dark`, `base16-mocha.dark`, `InspiredGitHub`, `Solarized (dark)`, `Solarized (light)`

**Catppuccin:** `catppuccin-latte`, `catppuccin-frappe`, `catppuccin-macchiato`, `catppuccin-mocha`

**Shiki / VS Code:** `andromeeda`, `aurora-x`, `ayu-dark`, `ayu-light`, `ayu-mirage`, `dark-plus`, `dracula`, `dracula-soft`, `everforest-dark`, `everforest-light`, `github-dark`, `github-dark-default`, `github-dark-dimmed`, `github-dark-high-contrast`, `github-light`, `github-light-default`, `github-light-high-contrast`, `gruvbox-dark-hard`, `gruvbox-dark-medium`, `gruvbox-dark-soft`, `gruvbox-light-hard`, `gruvbox-light-medium`, `gruvbox-light-soft`, `horizon`, `horizon-bright`, `houston`, `kanagawa-dragon`, `kanagawa-lotus`, `kanagawa-wave`, `laserwave`, `light-plus`, `material-theme`, `material-theme-darker`, `material-theme-lighter`, `material-theme-ocean`, `material-theme-palenight`, `min-dark`, `min-light`, `monokai`, `night-owl`, `night-owl-light`, `nord`, `one-dark-pro`, `one-light`, `plastic`, `poimandres`, `red`, `rose-pine`, `rose-pine-dawn`, `rose-pine-moon`, `slack-dark`, `slack-ochin`, `snazzy-light`, `solarized-dark`, `solarized-light`, `synthwave-84`, `tokyo-night`, `vesper`, `vitesse-black`, `vitesse-dark`, `vitesse-light`

### Divider

Visual separator line (horizontal or vertical).

```json
{
  "type": "divider",
  "direction": "horizontal",
  "thickness": 2,
  "line_style": "solid",
  "style": { "color": "#4B5563" }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `direction` | `enum` | `"horizontal"` | `"horizontal"` or `"vertical"` |
| `thickness` | `f32` | `2.0` | Line thickness in pixels |
| `line_style` | `enum` | `"solid"` | `"solid"`, `"dashed"`, `"dotted"` |
| `length` | `f32` | | Fixed length (omit for 100% of parent) |

Style: `color` (default `"#FFFFFF"`)

---

### Badge

Compact label with text and optional icon, pill-shaped.

```json
{
  "type": "badge",
  "text": "New",
  "icon": "lucide:star",
  "variant": "solid",
  "badge_size": "md",
  "style": { "background": "#3B82F6" }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `text` | `string` | (required) | Badge text |
| `icon` | `string` | | Iconify icon id (e.g. `"lucide:star"`) |
| `variant` | `enum` | `"solid"` | `"solid"` (filled) or `"outline"` (border only) |
| `badge_size` | `enum` | `"md"` | `"sm"`, `"md"`, `"lg"` |

Style: `background` (default `"#3B82F6"`) — badge color for solid variant or border color for outline, `font-size`, `font-family`

---

### Avatar

Circular image with optional border and status indicator.

```json
{
  "type": "avatar",
  "src": "photo.jpg",
  "size": 80,
  "border_color": "#3B82F6",
  "border_width": 3,
  "status": "online"
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `src` | `string` | (required) | Path to image file |
| `size` | `f32` | `64.0` | Diameter in pixels |
| `border_color` | `string` | | Border color (hex) |
| `border_width` | `f32` | `0.0` | Border thickness |
| `status` | `enum` | `"none"` | `"online"`, `"offline"`, `"away"`, `"none"` |
| `status_color` | `string` | | Override status dot color |

Status colors: online=#22C55E, offline=#9CA3AF, away=#F59E0B

---

### Callout

Speech bubble with directional arrow.

```json
{
  "type": "callout",
  "text": "Hello!",
  "arrow_direction": "bottom",
  "style": {
    "background": "#333333",
    "color": "#FFFFFF",
    "border-radius": 8,
    "font-size": 16
  }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `text` | `string` | (required) | Callout text |
| `arrow_direction` | `enum` | `"bottom"` | `"top"`, `"bottom"`, `"left"`, `"right"` |
| `arrow_size` | `f32` | `12.0` | Arrow triangle size |
| `size` | `{width, height}` | | Fixed size (auto-sized if omitted) |

Style: `background` (default `"#333333"`), `color` (default `"#FFFFFF"`), `border-radius` (default `8`), `font-size` (default `16`), `font-family`

---

### Terminal

Terminal/console window with colored lines and optional chrome.

```json
{
  "type": "terminal",
  "title": "Terminal",
  "theme": "dark",
  "reveal": { "mode": "typewriter", "start": 0.5, "duration": 3.0 },
  "lines": [
    { "text": "npm install", "line_type": "prompt" },
    { "text": "added 42 packages", "line_type": "output" }
  ],
  "size": { "width": 600, "height": 300 }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `lines` | `array` | (required) | `[{ "text", "line_type", "color" }]` |
| `theme` | `enum` | `"dark"` | `"dark"` or `"light"` |
| `title` | `string` | | Window title |
| `show_chrome` | `bool` | `true` | Show title bar with traffic light dots |
| `reveal` | `object` | | `{ "mode": "typewriter"\|"line_by_line", "start": 0, "duration": 1, "easing": "linear" }` |
| `size` | `{width, height}` | | Terminal size (default 500x auto) |

**Line types:** `"prompt"` (shows `$ ` prefix in green), `"command"` (white text), `"output"` (gray text)

**Reveal modes:** `"typewriter"` reveals characters one by one, `"line_by_line"` fades lines in sequentially.

---

### Table

Data table with headers and styled rows.

```json
{
  "type": "table",
  "headers": ["Name", "Role", "Status"],
  "rows": [
    ["Alice", "Engineer", "Active"],
    ["Bob", "Designer", "Away"]
  ],
  "size": { "width": 600, "height": 200 },
  "style": { "color": "#FFFFFF", "font-size": 14 }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `headers` | `string[]` | (required) | Column headers |
| `rows` | `string[][]` | (required) | Data rows |
| `header_color` | `string` | `"#374151"` | Header row background |
| `row_colors` | `string[]` | `["#1F2937", "#111827"]` | Alternating row colors |
| `border_color` | `string` | `"#4B5563"` | Grid line color |
| `header_text_color` | `string` | `"#FFFFFF"` | Header text color |
| `size` | `{width, height}` | | Table size |

Style: `color` (default `"#FFFFFF"`) — cell text color, `font-size` (default `14`), `font-family`, `border-radius`

---

### Chart

Data visualization with bar, line, or pie charts. Animated by default.

```json
{
  "type": "chart",
  "chart_type": "bar",
  "data": [
    { "value": 85, "label": "Q1" },
    { "value": 120, "label": "Q2" },
    { "value": 95, "label": "Q3" },
    { "value": 150, "label": "Q4" }
  ],
  "size": { "width": 400, "height": 300 }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `chart_type` | `enum` | (required) | `"bar"`, `"line"`, `"pie"` |
| `data` | `array` | (required) | `[{ "value", "label"?, "color"? }]` |
| `size` | `{width, height}` | `300x200` | Chart size |
| `animated` | `bool` | `true` | Animate chart fill/draw |
| `animation_duration` | `f64` | `1.5` | Animation duration in seconds |
| `colors` | `string[]` | | Custom color palette (hex) |

Default palette: `#3B82F6`, `#EF4444`, `#22C55E`, `#F59E0B`, `#8B5CF6`, `#EC4899`, `#06B6D4`, `#F97316`

---

### Mockup

Device frame (phone, laptop, browser) with image content inside.

```json
{
  "type": "mockup",
  "device": "iphone",
  "src": "screenshot.png",
  "theme": "dark",
  "size": { "width": 375, "height": 812 }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `device` | `enum` | (required) | `"iphone"`, `"android"`, `"laptop"`, `"browser"` |
| `src` | `string` | (required) | Path to content image |
| `theme` | `enum` | `"dark"` | `"dark"` or `"light"` bezel color |
| `size` | `{width, height}` | | Device size (defaults: iPhone 375x812, Android 360x800, Laptop 800x550, Browser 800x600) |

---

### Particle

Animated particle system for visual effects (confetti, snow, stars, bubbles).

```json
{
  "type": "particle",
  "particle_type": "confetti",
  "count": 80,
  "speed": 1.2,
  "seed": 42
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `particle_type` | `enum` | (required) | `"confetti"`, `"snow"`, `"stars"`, `"bubbles"`, `"halo"` |
| `count` | `u32` | `50` | Number of particles |
| `colors` | `string[]` | | Custom colors (defaults vary by type) |
| `speed` | `f32` | `1.0` | Speed multiplier |
| `size_range` | `{min, max}` | `{4, 12}` | Particle size range in pixels |
| `seed` | `u64` | `42` | Random seed for reproducible results |

**Particle behaviors:**
- **confetti**: colored rectangles falling with rotation and horizontal wobble
- **snow**: white circles falling gently with lateral drift
- **stars**: fixed positions with twinkling opacity
- **bubbles**: semi-transparent circles rising with oscillation
- **halo**: soft glowing circles drifting slowly with pulsing opacity (great for backgrounds)

---

## Animations

All animation effects are defined inside `style.animation` as a **typed array**, each discriminated by `"name"`. A single effect (without array) is also accepted.

```json
{
  "style": {
    "animation": [
      { "name": "fade_in_up", "delay": 0.2, "duration": 0.8 },
      { "name": "glow", "color": "#6366F1", "radius": 20, "intensity": 2.0 },
      { "name": "wiggle", "property": "translate_y", "amplitude": 5, "frequency": 0.8, "seed": 42 }
    ]
  }
}
```

### Effect Types

| Effect name | Fields | Description |
|---|---|---|
| *preset name* | `delay`, `duration`, `loop` | Any of the 31 presets (e.g. `fade_in_up`, `scale_in`) |
| `glow` | `color`, `radius`, `intensity` | Luminous halo effect |
| `wiggle` | `property`, `amplitude`, `frequency`, `mode`, `seed`, ... | Procedural noise animation |
| `keyframes` | `keyframes` | Custom keyframe animations |
| `motion_blur` | `intensity` | Motion blur effect |

### Animation Presets

```json
{
  "style": {
    "animation": [{ "name": "fade_in_up", "delay": 0.2, "duration": 0.8, "loop": false }]
  }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `delay` | `f64` | `0.0` | Delay before animation starts (seconds) |
| `duration` | `f64` | `0.8` | Animation duration (seconds) |
| `loop` | `bool` | `false` | Loop the animation continuously |

#### Entrance Presets

| Preset | Description |
|---|---|
| `fade_in` | Fade from transparent |
| `fade_in_up` | Fade in + slide up |
| `fade_in_down` | Fade in + slide down |
| `fade_in_left` | Fade in + slide from left |
| `fade_in_right` | Fade in + slide from right |
| `slide_in_left` | Slide in from far left |
| `slide_in_right` | Slide in from far right |
| `slide_in_up` | Slide in from below |
| `slide_in_down` | Slide in from above |
| `scale_in` | Scale up from 0 with spring bounce |
| `bounce_in` | Bouncy scale from small to normal |
| `blur_in` | Fade in from blurred |
| `rotate_in` | Rotate + scale from half size |
| `elastic_in` | Elastic underdamped spring scale |

#### Exit Presets

| Preset | Description |
|---|---|
| `fade_out` | Fade to transparent |
| `fade_out_up` | Fade out + slide up |
| `fade_out_down` | Fade out + slide down |
| `slide_out_left` | Slide out to the left |
| `slide_out_right` | Slide out to the right |
| `slide_out_up` | Slide out upward |
| `slide_out_down` | Slide out downward |
| `scale_out` | Scale down to 0 |
| `bounce_out` | Bouncy scale to small |
| `blur_out` | Fade out with blur |
| `rotate_out` | Rotate + scale to half size |

#### Continuous Presets

Use `"loop": true` for continuous animation:

| Preset | Description |
|---|---|
| `pulse` | Gentle scale oscillation |
| `float` | Vertical floating motion |
| `shake` | Horizontal shake |
| `spin` | 360-degree continuous rotation |

#### Special Presets

| Preset | Description |
|---|---|
| `typewriter` | Progressive character reveal |
| `wipe_left` | Slide in from left with fade |
| `wipe_right` | Slide in from right with fade |

### Custom Keyframe Animations

```json
{
  "style": {
    "animation": [
      {
        "name": "keyframes",
        "keyframes": [
          {
            "property": "opacity",
            "keyframes": [
              { "time": 0.0, "value": 0.0 },
              { "time": 0.5, "value": 1.0 }
            ],
            "easing": "ease_out"
          }
        ]
      }
    ]
  }
}
```

**Animatable properties:** `opacity`, `translate_x`, `translate_y`, `scale_x`, `scale_y`, `scale` (both axes), `rotation`, `blur`, `color`

**11 easing functions:** `linear`, `ease_in`, `ease_out`, `ease_in_out`, `ease_in_quad`, `ease_out_quad`, `ease_in_cubic`, `ease_out_cubic`, `ease_in_expo`, `ease_out_expo`, `spring`

**Spring physics** (when easing is `spring`):
```json
{
  "easing": "spring",
  "spring": { "damping": 15, "stiffness": 100, "mass": 1 }
}
```

### Glow

```json
{
  "style": {
    "animation": [
      { "name": "glow", "color": "#ff00ff", "radius": 20, "intensity": 2.5 }
    ]
  }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `color` | `string` | `"#FFFFFF"` | Glow color (hex) |
| `radius` | `f32` | `10.0` | Blur radius |
| `intensity` | `f32` | `1.0` | Brightness multiplier |

### Wiggle (Procedural Noise)

Wiggle adds continuous organic movement. Offsets are applied **additively** on top of presets and keyframes.

```json
{
  "style": {
    "animation": [
      { "name": "wiggle", "property": "translate_x", "amplitude": 5, "frequency": 3, "seed": 42 },
      { "name": "wiggle", "property": "rotation", "amplitude": 2, "frequency": 2, "seed": 99, "decay": 0.5 }
    ]
  }
}
```

| Field | Type | Default | Description |
|---|---|---|---|
| `property` | `string` | (required) | Property to wiggle (same as animatable properties) |
| `amplitude` | `f64` | (required) | Maximum deviation (pixels for translate, degrees for rotation) |
| `frequency` | `f64` | (required) | Cycles per second (Hz) |
| `mode` | `string` | `"noise"` | `"noise"` (layered simplex) or `"sine"` (pure sine wave) |
| `seed` | `u64` | `0` | Random seed for reproducible results (noise mode only) |
| `octaves` | `u32` | `3` | Noise complexity (noise mode only) |
| `phase` | `f64` | `0.0` | Phase offset |
| `decay` | `f64` | | Exponential decay rate |
| `easing` | `string` | | Remap noise through an easing curve |

### Motion Blur

```json
{
  "style": {
    "animation": [
      { "name": "motion_blur", "intensity": 0.8 }
    ]
  }
}
```

Renders multiple sub-frames and composites them for physically-correct motion blur.

---

## Output Formats

| Format | Command | Requires |
|---|---|---|
| **MP4 (H.264)** | `rustmotion render in.json -o out.mp4` | Built-in |
| **MP4 (H.265)** | `rustmotion render in.json -o out.mp4 --codec h265` | ffmpeg |
| **WebM (VP9)** | `rustmotion render in.json -o out.webm --codec vp9` | ffmpeg |
| **MOV (ProRes)** | `rustmotion render in.json -o out.mov --codec prores` | ffmpeg |
| **Animated GIF** | `rustmotion render in.json -o out.gif --format gif` | Built-in |
| **PNG Sequence** | `rustmotion render in.json -o frames/ --format png-seq` | Built-in |
| **Single Frame** | `rustmotion render in.json --frame 0 -o preview.png` | Built-in |

Transparency is supported with `--transparent` for PNG sequences, WebM (VP9), and ProRes 4444.

---

## Full Example

```json
{
  "version": "1.0",
  "video": {
    "width": 1080,
    "height": 1920,
    "fps": 30,
    "background": "#0f172a"
  },
  "scenes": [
    {
      "duration": 4.0,
      "layout": { "align_items": "center", "justify_content": "center", "gap": 32 },
      "children": [
        {
          "type": "shape",
          "shape": "rounded_rect",
          "size": { "width": 900, "height": 520 },
          "style": {
            "fill": {
              "type": "linear",
              "colors": ["#6366f1", "#8b5cf6"],
              "angle": 135
            },
            "border-radius": 32,
            "animation": [{ "name": "scale_in", "duration": 0.6 }]
          }
        },
        {
          "type": "icon",
          "icon": "lucide:rocket",
          "size": { "width": 80, "height": 80 },
          "style": {
            "color": "#FFFFFF",
            "animation": [{ "name": "fade_in_up", "delay": 0.3, "duration": 0.6 }]
          }
        },
        {
          "type": "text",
          "content": "Ship Faster",
          "style": {
            "font-size": 64,
            "color": "#FFFFFF",
            "font-weight": "bold",
            "text-align": "center",
            "animation": [{ "name": "fade_in_up", "delay": 0.5, "duration": 0.6 }]
          }
        },
        {
          "type": "text",
          "content": "Build motion videos in Rust.\nNo browser needed.",
          "max_width": 700,
          "style": {
            "font-size": 32,
            "color": "#CBD5E1",
            "text-align": "center",
            "line-height": 1.5,
            "animation": [{ "name": "fade_in_up", "delay": 0.7, "duration": 0.6 }]
          }
        }
      ]
    },
    {
      "duration": 3.0,
      "background": "#1e293b",
      "transition": { "type": "iris", "duration": 0.8 },
      "layout": { "align_items": "center", "justify_content": "center" },
      "children": [
        {
          "type": "text",
          "content": "No browser needed.",
          "style": {
            "font-size": 56,
            "color": "#e2e8f0",
            "text-align": "center",
            "animation": [{ "name": "typewriter", "duration": 1.5 }]
          }
        }
      ]
    }
  ]
}
```

## Architecture

- **Rendering:** skia-safe (same engine as Chrome/Flutter)
- **Video encoding:** openh264 (Cisco BSD, compiled from source) + ffmpeg (optional, for H.265/VP9/ProRes)
- **Audio encoding:** AAC via minimp4
- **SVG rendering:** resvg + usvg
- **Icon rendering:** Iconify API (200k+ icons)
- **GIF decoding/encoding:** gif crate
- **MP4 muxing:** minimp4
- **JSON Schema:** schemars (auto-generated from Rust types)
- **Parallelism:** rayon (multi-threaded frame rendering)

## License

MIT
