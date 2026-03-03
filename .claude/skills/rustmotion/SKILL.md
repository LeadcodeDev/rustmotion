---
name: rustmotion
description: Best practices for Rustmotion - Video creation in Rust
metadata:
  tags: motion, video, rust, animation, composition
---

# Skill: Generate rustmotion JSON Scenarios

## What is rustmotion?

rustmotion is a CLI tool that renders motion design videos from JSON scenario files. It uses Skia for 2D rendering and supports MP4, WebM, MOV, GIF, and PNG sequence outputs.

## JSON Scenario Structure

A scenario is a JSON object with:

```json
{
  "version": "1.0",
  "video": { ... },
  "audio": [ ... ],
  "scenes": [ ... ]
}
```

### `video` (required)

| Field        | Type   | Default     | Description                                             |
| ------------ | ------ | ----------- | ------------------------------------------------------- |
| `width`      | u32    | required    | Video width in pixels. **Must be even for H.264.**      |
| `height`     | u32    | required    | Video height in pixels. **Must be even for H.264.**     |
| `fps`        | u32    | `30`        | Frames per second                                       |
| `background` | string | `"#000000"` | Default background color (hex `#RRGGBB` or `#RRGGBBAA`) |
| `codec`      | string | `null`      | `"h264"`, `"h265"`, `"vp9"`, `"prores"`                 |
| `crf`        | u8     | `null`      | Constant Rate Factor (0-51, lower = better quality)     |

### `audio` (optional array)

| Field      | Type   | Default  | Description                                   |
| ---------- | ------ | -------- | --------------------------------------------- |
| `src`      | string | required | Path to audio file (wav, mp3, ogg, flac, aac) |
| `start`    | f64    | `0`      | Start time in seconds                         |
| `end`      | f64    | `null`   | End time (null = full duration)               |
| `volume`   | f32    | `1.0`    | Volume multiplier                             |
| `fade_in`  | f64    | `null`   | Fade in duration in seconds                   |
| `fade_out` | f64    | `null`   | Fade out duration in seconds                  |

### `scenes` (required array)

| Field        | Type   | Default  | Description                                    |
| ------------ | ------ | -------- | ---------------------------------------------- |
| `duration`   | f64    | required | Scene duration in seconds (must be > 0)        |
| `background` | string | `null`   | Override video background for this scene       |
| `layers`     | array  | `[]`     | Layers rendered in order (first = back)        |
| `transition` | object | `null`   | Transition to this scene from the previous one |
| `freeze_at`  | f64    | `null`   | Freeze the scene at this time (seconds)        |

### Transitions

```json
{ "type": "fade", "duration": 0.5 }
```

**13 transition types:** `fade`, `wipe_left`, `wipe_right`, `wipe_up`, `wipe_down`, `zoom_in`, `zoom_out`, `flip`, `clock_wipe`, `iris`, `slide`, `dissolve`, `none`

Default transition duration: `0.5` seconds.

---

## Layer Types

All layers are discriminated by the `"type"` field. Layers are rendered in array order (first = bottom).

### Common optional fields (on most layers)

| Field           | Type   | Default | Description                                      |
| --------------- | ------ | ------- | ------------------------------------------------ |
| `opacity`       | f32    | `1.0`   | 0.0 to 1.0                                       |
| `animations`    | array  | `[]`    | Custom keyframe animations                       |
| `preset`        | string | `null`  | Animation preset name                            |
| `preset_config` | object | `null`  | `{ "delay": 0, "duration": 0.8, "loop": false }` |
| `start_at`      | f64    | `null`  | Show layer starting at this time (seconds)       |
| `end_at`        | f64    | `null`  | Hide layer after this time (seconds)             |
| `wiggle`        | array  | `null`  | Procedural noise-based animation                 |
| `motion_blur`   | f32    | `null`  | Motion blur intensity                            |

### 1. `text`

```json
{
  "type": "text",
  "content": "Hello World",
  "position": { "x": 100, "y": 100 },
  "font_size": 48,
  "color": "#FFFFFF",
  "font_family": "Arial",
  "font_weight": "bold",
  "align": "center",
  "max_width": 800,
  "line_height": 1.2,
  "letter_spacing": 2.0
}
```

| Field            | Type     | Default                                               |
| ---------------- | -------- | ----------------------------------------------------- |
| `content`        | string   | required                                              |
| `position`       | `{x, y}` | `{0, 0}`                                              |
| `font_size`      | f32      | `24.0`                                                |
| `color`          | string   | `"#FFFFFF"`                                           |
| `font_family`    | string   | `"Arial"`                                             |
| `font_weight`    | enum     | `"normal"` — options: `"normal"`, `"bold"`, `"light"` |
| `align`          | enum     | `"left"` — options: `"left"`, `"center"`, `"right"`   |
| `max_width`      | f32      | `null`                                                |
| `line_height`    | f32      | `null`                                                |
| `letter_spacing` | f32      | `null`                                                |

### 2. `shape`

```json
{
  "type": "shape",
  "shape": "rounded_rect",
  "position": { "x": 50, "y": 50 },
  "size": { "width": 200, "height": 100 },
  "fill": "#FF5733",
  "corner_radius": 16,
  "stroke": { "color": "#FFFFFF", "width": 2 }
}
```

| Field           | Type               | Default                            |
| --------------- | ------------------ | ---------------------------------- |
| `shape`         | enum               | required                           |
| `position`      | `{x, y}`           | `{0, 0}`                           |
| `size`          | `{width, height}`  | `{100, 100}`                       |
| `fill`          | string or gradient | `null`                             |
| `stroke`        | `{color, width}`   | `null`                             |
| `corner_radius` | f32                | `null` (for `rounded_rect`)        |
| `text`          | object             | `null` — embedded text (see below) |

**Shape types:** `rect`, `circle`, `rounded_rect`, `ellipse`, `triangle`, `star` (with `points`, default 5), `polygon` (with `sides`, default 6), `path` (with `data` SVG path string)

**Fill can be a gradient:**

```json
{
  "fill": {
    "type": "linear",
    "colors": ["#FF0000", "#0000FF"],
    "angle": 45,
    "stops": [0.0, 1.0]
  }
}
```

Gradient types: `linear`, `radial`.

**Embedded text in shapes (`text` field):**

```json
{
  "text": {
    "content": "Click me",
    "font_size": 16,
    "color": "#FFFFFF",
    "font_family": "Arial",
    "font_weight": "bold",
    "align": "center",
    "vertical_align": "center"
  }
}
```

`vertical_align`: `"top"`, `"center"`, `"bottom"` (default: `"center"`).

### 3. `image`

```json
{
  "type": "image",
  "src": "path/to/image.png",
  "position": { "x": 0, "y": 0 },
  "size": { "width": 400, "height": 300 },
  "fit": "cover"
}
```

| Field      | Type              | Default                                                         |
| ---------- | ----------------- | --------------------------------------------------------------- |
| `src`      | string            | required — path to image file                                   |
| `position` | `{x, y}`          | `{0, 0}`                                                        |
| `size`     | `{width, height}` | `null` (uses image dimensions)                                  |
| `fit`      | enum              | `"cover"` — options: `"cover"`, `"contain"`, `"fill"`, `"none"` |

### 4. `svg`

```json
{
  "type": "svg",
  "data": "<svg>...</svg>",
  "position": { "x": 0, "y": 0 },
  "size": { "width": 200, "height": 200 }
}
```

| Field      | Type              | Default                                                     |
| ---------- | ----------------- | ----------------------------------------------------------- |
| `src`      | string            | `null` — path to SVG file (either `src` or `data` required) |
| `data`     | string            | `null` — inline SVG markup                                  |
| `position` | `{x, y}`          | `{0, 0}`                                                    |
| `size`     | `{width, height}` | `null`                                                      |

### 5. `video`

```json
{
  "type": "video",
  "src": "path/to/video.mp4",
  "position": { "x": 0, "y": 0 },
  "size": { "width": 1920, "height": 1080 },
  "trim_start": 2.0,
  "trim_end": 10.0
}
```

| Field           | Type              | Default   |
| --------------- | ----------------- | --------- |
| `src`           | string            | required  |
| `position`      | `{x, y}`          | `{0, 0}`  |
| `size`          | `{width, height}` | required  |
| `trim_start`    | f64               | `null`    |
| `trim_end`      | f64               | `null`    |
| `playback_rate` | f64               | `null`    |
| `fit`           | enum              | `"cover"` |
| `volume`        | f32               | `1.0`     |
| `loop_video`    | bool              | `null`    |

### 6. `gif`

```json
{
  "type": "gif",
  "src": "path/to/animation.gif",
  "position": { "x": 100, "y": 100 },
  "size": { "width": 200, "height": 200 }
}
```

| Field      | Type              | Default   |
| ---------- | ----------------- | --------- |
| `src`      | string            | required  |
| `position` | `{x, y}`          | `{0, 0}`  |
| `size`     | `{width, height}` | `null`    |
| `fit`      | enum              | `"cover"` |
| `loop_gif` | bool              | `true`    |

### 7. `caption`

Timed word-by-word captions with active word highlighting.

```json
{
  "type": "caption",
  "words": [
    { "text": "Hello", "start": 0.0, "end": 0.5 },
    { "text": "World", "start": 0.5, "end": 1.0 }
  ],
  "position": { "x": 540, "y": 1600 },
  "font_size": 48,
  "color": "#FFFFFF",
  "active_color": "#FFD700",
  "style": "highlight",
  "max_width": 900
}
```

| Field          | Type     | Default                                                                    |
| -------------- | -------- | -------------------------------------------------------------------------- |
| `words`        | array    | required — `[{ "text", "start", "end" }]`                                  |
| `position`     | `{x, y}` | `{0, 0}`                                                                   |
| `font_size`    | f32      | `24.0`                                                                     |
| `font_family`  | string   | `null`                                                                     |
| `color`        | string   | `"#FFFFFF"`                                                                |
| `active_color` | string   | `"#FFD700"`                                                                |
| `background`   | string   | `null`                                                                     |
| `style`        | enum     | `"default"` — options: `"default"`, `"highlight"`, `"karaoke"`, `"bounce"` |
| `max_width`    | f32      | `null`                                                                     |

### 8. `group`

Groups multiple layers with shared position and animations.

```json
{
  "type": "group",
  "position": { "x": 100, "y": 100 },
  "layers": [ ... ],
  "preset": "fade_in"
}
```

| Field      | Type     | Default              |
| ---------- | -------- | -------------------- |
| `position` | `{x, y}` | `{0, 0}`             |
| `layers`   | array    | `[]` — nested layers |

---

## Animations

### Custom Keyframe Animations

```json
{
  "animations": [
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

### Animation Presets

Use `"preset"` instead of manual keyframes:

```json
{
  "type": "text",
  "content": "Hello",
  "position": { "x": 540, "y": 500 },
  "preset": "fade_in_up",
  "preset_config": { "delay": 0.2, "duration": 0.8, "loop": false }
}
```

**31 presets:**

| Category   | Presets                                                                                                                                                                                                    |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Entrances  | `fade_in`, `fade_in_up`, `fade_in_down`, `fade_in_left`, `fade_in_right`, `slide_in_left`, `slide_in_right`, `slide_in_up`, `slide_in_down`, `scale_in`, `bounce_in`, `blur_in`, `rotate_in`, `elastic_in` |
| Exits      | `fade_out`, `fade_out_up`, `fade_out_down`, `slide_out_left`, `slide_out_right`, `slide_out_up`, `slide_out_down`, `scale_out`, `bounce_out`, `blur_out`, `rotate_out`                                     |
| Continuous | `pulse`, `float`, `shake`, `spin` (use `"loop": true` in preset_config)                                                                                                                                    |
| Special    | `typewriter`, `wipe_left`, `wipe_right`                                                                                                                                                                    |

### Wiggle (Procedural Noise)

```json
{
  "wiggle": [
    { "property": "translate_x", "amplitude": 5, "frequency": 3, "seed": 42 }
  ]
}
```

---

## Minimal Complete Example

```json
{
  "video": {
    "width": 1080,
    "height": 1920,
    "fps": 30,
    "background": "#0f172a"
  },
  "scenes": [
    {
      "duration": 3.0,
      "layers": [
        {
          "type": "shape",
          "shape": "rounded_rect",
          "position": { "x": 140, "y": 760 },
          "size": { "width": 800, "height": 400 },
          "fill": {
            "type": "linear",
            "colors": ["#6366f1", "#8b5cf6"],
            "angle": 135
          },
          "corner_radius": 24,
          "preset": "scale_in"
        },
        {
          "type": "text",
          "content": "Hello rustmotion!",
          "position": { "x": 540, "y": 940 },
          "font_size": 56,
          "color": "#FFFFFF",
          "font_weight": "bold",
          "align": "center",
          "preset": "fade_in_up",
          "preset_config": { "delay": 0.3, "duration": 0.8 }
        }
      ]
    },
    {
      "duration": 2.0,
      "transition": { "type": "fade", "duration": 0.5 },
      "layers": [
        {
          "type": "text",
          "content": "Powered by Rust + Skia",
          "position": { "x": 540, "y": 960 },
          "font_size": 36,
          "color": "#94a3b8",
          "align": "center",
          "preset": "fade_in"
        }
      ]
    }
  ]
}
```

---

## Validation obligatoire du JSON généré

**Après chaque génération de JSON**, tu DOIS valider le scénario avant de le présenter à l'utilisateur :

1. **Écrire** le JSON dans un fichier temporaire (ex: `/tmp/scenario.json`)
2. **Exécuter** `rustmotion validate /tmp/scenario.json` pour vérifier la validité
3. **Si la validation échoue** : corriger les erreurs et re-valider jusqu'à ce que ça passe
4. **Si la validation réussit** : présenter le JSON à l'utilisateur

Ne jamais proposer un JSON qui n'a pas été validé par `rustmotion validate`.

---

## CLI Commands

```bash
# Render a scenario file to MP4
rustmotion render scenario.json -o output.mp4

# Render from inline JSON
rustmotion render --json '{ ... }' -o output.mp4

# Validate a scenario without rendering
rustmotion validate scenario.json

# Print the JSON Schema
rustmotion schema

# Show scenario info
rustmotion info scenario.json

# Render a single frame (0-indexed) as PNG
rustmotion render scenario.json -o frame.png --frame 0

# Render with specific codec/format
rustmotion render scenario.json -o output.webm --codec vp9 --format webm

# Render as GIF
rustmotion render scenario.json -o output.gif --format gif

# Render as PNG sequence
rustmotion render scenario.json -o frames/ --format png-seq

# Machine-readable output
rustmotion render scenario.json -o output.mp4 --output-format json
```

---

## Important Constraints

1. **Even dimensions**: `width` and `height` must be even numbers for H.264 encoding
2. **Common resolutions**: 1080x1920 (portrait 9:16), 1920x1080 (landscape 16:9), 1080x1080 (square)
3. **Timing**: `start_at` must be < `end_at` when both are specified
4. **Layer order**: Layers render bottom-to-top (first layer in array = behind)
5. **File paths**: Image, video, GIF, SVG `src` paths are relative to the working directory
6. **SVG layers**: Must have either `src` or `data`, not both empty
7. **At least one scene** is required, each with `duration > 0`
8. **Colors**: Use hex format `#RRGGBB` or `#RRGGBBAA`
9. **Presets vs animations**: `preset` is a shorthand; explicit `animations` override preset animations on the same property
10. **Continuous presets** (`pulse`, `float`, `shake`, `spin`) should use `"loop": true` in `preset_config`
