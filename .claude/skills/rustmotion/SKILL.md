---
name: rustmotion
description: Best practices for Rustmotion - Video creation in Rust
metadata:
  tags: motion, video, rust, animation, composition
---

# Skill: Generate rustmotion JSON Scenarios

## What is rustmotion?

rustmotion is a CLI tool that renders motion design videos from JSON scenario files. It uses Skia for 2D rendering and supports MP4, WebM, MOV, GIF, and PNG sequence outputs.

## Quick Reference

**Common resolutions:** 1080x1920 (portrait 9:16), 1920x1080 (landscape 16:9), 1080x1080 (square)

**Essential CLI:**
```bash
rustmotion validate scenario.json          # Validate without rendering
rustmotion render scenario.json -o out.mp4 # Render to MP4
rustmotion render scenario.json -o f.png --frame 0  # Single frame
rustmotion schema                          # Print JSON Schema
```

**JSON skeleton:**
```json
{
  "version": "1.0",
  "video": { "width": 1080, "height": 1920, "fps": 30, "background": "#0f172a" },
  "scenes": [
    { "duration": 3.0, "children": [ ... ] }
  ]
}
```

---

## Video Creation Wizard

When the user provides a **video idea or subject** (not a technical question), activate this guided wizard flow. Examples of triggers: "je veux créer une vidéo pour...", "make a video about...", "une vidéo de présentation de...", or any prompt describing video content to produce.

### Phase 1: Brief global

Ask the user **3-5 questions** using `AskUserQuestion` to understand the project. Ask them one at a time or grouped logically:

1. **Format & Device** — Portrait 9:16 / Mobile (1080×1920), Landscape 16:9 / Desktop (1920×1080), or Square 1:1 (1080×1080)? Accept aliases: "mobile"/"phone"/"story"/"reel"/"TikTok" → Mobile 9:16, "desktop"/"YouTube"/"presentation" → Desktop 16:9, "tablet"/"iPad" → Tablet. **The chosen device determines all component sizing** — see [rules/responsive-device-sizing.md](rules/responsive-device-sizing.md).
2. **Target duration** — Short (15-30s), Medium (30-60s), or Long (60s+)?
3. **Tone/style** — Corporate, Playful, Minimal, Tech/Dark, Colorful?
4. **Key content** — What text, data, features, or CTA should appear?
5. **Color palette** — Brand colors, or should we suggest one?

Skip questions where the answer is already obvious from context.

### Phase 2: Scene plan with component suggestions

Based on the brief, propose a **textual scene plan** that maps the user's ideas to concrete rustmotion components. Format:

```
Scene 1 (3s) : Intro
  → icon (lucide:rocket) + text (tagline) with char_scale_in
  → animated-background radial gradient (#0f172a → #1e1b4b)
  → particle stars for ambiance

Scene 2 (4s) : The Problem
  → badge "Pain Point" + main text with char_fade_in (granularity: word)
  → 3x card in row with icons (stagger 0.2s)
  → dark red background with concentric_circles
```

Each scene must include:
- **Concrete components** (text, card, icon, shape, badge, counter, etc.)
- **Recommended animations** (presets, char animations, glow, wiggle)
- **Adapted background** (gradient, particles, concentric_circles)
- **Suggested icons** (lucide:xxx, simple-icons:xxx)

**Idea → Component mapping table:**

| User's idea | Recommended components |
|---|---|
| Stats / numbers | `counter` (animated) + `card` |
| Features / benefits | `card` grid + `icon` + `badge` |
| Code / technical | `codeblock` + `terminal` |
| Process / steps | `timeline` component |
| Comparison | `flex` row with 2 `card` side by side |
| Testimonial | `card` with `shape` circle (avatar) + `text` italic |
| Pricing | `card` with `counter` + `text` |
| Partner logos | `flex` row + `icon` (simple-icons:xxx) |
| CTA / call to action | `badge` + glow + `particle` confetti |
| Hero / intro | `text` with `char_scale_in` + main `icon` |
| Transition / ambiance | `particle` stars/confetti + `animated-background` |

The user validates or adjusts the plan before proceeding.

### Phase 3: Iterative scene-by-scene construction

For each scene in the validated plan:
1. Generate the JSON for the scene
2. Add the scene to the global JSON file (named after the subject in kebab-case, e.g. `saas-analytics-presentation.json`)
3. Validate with `rustmotion validate`
4. Optionally propose a preview (`rustmotion render --frame N`) for visually complex scenes
5. The user validates or requests adjustments
6. Move to the next scene

**Important:** Always write incrementally. Never generate the entire video at once.

### Phase 4: Finalization

1. Assemble the complete JSON with all scenes
2. Run final `rustmotion validate`
3. Render with `rustmotion render -o output.mp4 --quiet`
4. Suggest `--codec prores` for videos with dark gradients

### Design guidelines

- **Scene duration:** 3-5s per scene is the sweet spot. Intro/outro can be shorter (2-3s).
- **Animation patterns:** Stagger entrances within a scene (0.1-0.3s delays). Use fade/slide transitions between scenes.
- **Backgrounds:** Radial gradient for dark themes, concentric_circles for tech feel, particles for ambiance.
- **Visual hierarchy:** Title (large font) → subtitle (medium) → body (smaller). Use color contrast to guide the eye.
- **Consistency:** Keep the same color palette and animation style across all scenes.
- **Pacing:** Alternate between dense scenes (multiple elements) and breathing scenes (single focal point).
- **Device-aware sizing:** All component sizes (font-size, icon size, card width, padding, gaps) MUST be scaled to the target device. Use the Tailwind 4 type scale as reference, multiplied by the device factor (×3 for mobile, ×1.5 for desktop, ×2.5 for square). See [rules/responsive-device-sizing.md](rules/responsive-device-sizing.md). A title on mobile should be `text-4xl` equivalent = 108px, NOT 48px.

---

## Rules

Read individual rule files for detailed explanations, GOOD/BAD examples, and constraints:

- [rules/validate-json.md](rules/validate-json.md) - Always validate generated JSON with `rustmotion validate` before presenting
- [rules/even-dimensions.md](rules/even-dimensions.md) - Use even width/height for H.264 encoding
- [rules/counter-standalone.md](rules/counter-standalone.md) - Counter must be standalone (no baseline correction inside cards)
- [rules/vertical-align.md](rules/vertical-align.md) - Shape text vertical_align: use "top"/"middle"/"bottom" (NOT "center")
- [rules/stagger-animations.md](rules/stagger-animations.md) - Stagger animations with increasing style.animation.delay
- [rules/layer-order.md](rules/layer-order.md) - Layer order matters: first in array = behind, last = front
- [rules/card-flex-layout.md](rules/card-flex-layout.md) - Scene = implicit flex container; use card/flex for nested layout
- [rules/continuous-presets.md](rules/continuous-presets.md) - Continuous presets (pulse, float, shake, spin) need loop: true
- [rules/timing-constraints.md](rules/timing-constraints.md) - Timing: start_at must be < end_at, duration > 0
- [rules/icon-format.md](rules/icon-format.md) - Icons use Iconify (200k+ icons), format "prefix:name" (e.g. "lucide:home")
- [rules/grid-card-height.md](rules/grid-card-height.md) - Grid containers need explicit height (not "auto") to prevent row stretching
- [rules/wiggle-additive.md](rules/wiggle-additive.md) - Wiggle is additive on top of presets and keyframes
- [rules/prefer-presets.md](rules/prefer-presets.md) - Prefer presets over manual keyframes (39 built-in presets)
- [rules/hex-colors.md](rules/hex-colors.md) - Colors in hex format only (#RRGGBB or #RRGGBBAA)
- [rules/easing-guidelines.md](rules/easing-guidelines.md) - Easing guidelines for motion design
- [rules/text-background.md](rules/text-background.md) - text-background renders a colored rectangle behind text
- [rules/3d-perspective.md](rules/3d-perspective.md) - 3D perspective transforms with rotate_x, rotate_y, perspective keyframes
- [rules/timeline-sequencing.md](rules/timeline-sequencing.md) - Timeline steps for multi-phase animations within a single scene
- [rules/gradient-quality.md](rules/gradient-quality.md) - Gradient quality: linear color space, 10-bit encoding, ProRes for dark gradients
- [rules/video-wizard.md](rules/video-wizard.md) - Video creation wizard: iterative scene-by-scene construction best practices
- [rules/responsive-device-sizing.md](rules/responsive-device-sizing.md) - CRITICAL: Scale all sizes to target device using Tailwind 4 type scale (×3 mobile, ×1.5 desktop)

---

## Complete Examples

### Example 1: Marketing Card (Portrait)

```json
{
  "version": "1.0",
  "video": { "width": 1080, "height": 1920, "fps": 30, "background": "#0f172a" },
  "scenes": [
    {
      "duration": 4.0,
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
    }
  ]
}
```

### Example 2: Code Tutorial (Landscape)

```json
{
  "version": "1.0",
  "video": { "width": 1920, "height": 1080, "fps": 30, "background": "#1a1b26" },
  "scenes": [
    {
      "duration": 10.0,
      "children": [
        {
          "type": "text",
          "content": "Getting Started with Rust",
          "style": {
            "font-size": 40,
            "color": "#7AA2F7",
            "font-weight": "bold",
            "text-align": "center",
            "animation": [{ "name": "fade_in", "duration": 0.5 }]
          }
        },
        {
          "type": "codeblock",
          "code": "fn main() {\n    println!(\"Hello, world!\");\n}",
          "language": "rust",
          "theme": "tokyo-night",
          "size": { "width": 1400, "height": 400 },
          "show_line_numbers": true,
          "chrome": { "enabled": true, "title": "src/main.rs" },
          "reveal": { "mode": "typewriter", "start": 0.5, "duration": 3.0 },
          "style": { "font-size": 22, "padding": 24, "border-radius": 16 },
          "states": [
            {
              "code": "fn main() {\n    let name = \"rustmotion\";\n    println!(\"Hello, {}!\", name);\n}",
              "at": 5.0,
              "duration": 2.5,
              "cursor": { "enabled": true, "blink": true }
            }
          ]
        },
        {
          "type": "text",
          "content": "Variables are immutable by default",
          "start_at": 5.0,
          "style": {
            "font-size": 28,
            "color": "#9ECE6A",
            "text-align": "center",
            "animation": [{ "name": "fade_in_up", "delay": 0.0, "duration": 0.6 }]
          }
        }
      ]
    }
  ]
}
```

### Example 3: Multi-Scene with Transitions

```json
{
  "version": "1.0",
  "video": { "width": 1080, "height": 1920, "fps": 30, "background": "#0a0a14" },
  "scenes": [
    {
      "duration": 3.0,
      "children": [
        {
          "type": "text",
          "content": "2024 Results",
          "style": { "font-size": 72, "color": "#FFFFFF", "font-weight": "bold", "text-align": "center", "animation": [{ "name": "fade_in_up" }] }
        },
        {
          "type": "text",
          "content": "Year in Review",
          "style": { "font-size": 36, "color": "#94A3B8", "text-align": "center", "animation": [{ "name": "fade_in_up", "delay": 0.3, "duration": 0.6 }] }
        }
      ]
    },
    {
      "duration": 4.0,
      "transition": { "type": "slide", "duration": 0.6 },
      "children": [
        {
          "type": "counter",
          "from": 0,
          "to": 12500,
          "separator": ",",
          "easing": "ease_out",
          "start_at": 0.3,
          "end_at": 3.5,
          "style": { "font-size": 96, "color": "#38BDF8", "font-weight": "bold", "text-align": "center" }
        },
        {
          "type": "text",
          "content": "Users Reached",
          "style": { "font-size": 36, "color": "#CBD5E1", "text-align": "center", "animation": [{ "name": "fade_in_up", "delay": 0.5, "duration": 0.6 }] }
        },
        {
          "type": "card",
          "size": { "width": 900, "height": "auto" },
          "style": {
            "flex-direction": "row",
            "gap": 16,
            "padding": 24,
            "background": "#1E293B",
            "border-radius": 20,
            "animation": [{ "name": "fade_in_up", "delay": 0.8, "duration": 0.6 }]
          },
          "children": [
            {
              "type": "icon",
              "icon": "lucide:trending-up",
              "size": { "width": 48, "height": 48 },
              "style": { "color": "#22C55E" }
            },
            {
              "type": "text",
              "content": "+340% growth YoY",
              "style": { "font-size": 32, "color": "#FFFFFF", "font-weight": "bold" }
            }
          ]
        }
      ]
    },
    {
      "duration": 3.0,
      "transition": { "type": "fade", "duration": 0.5 },
      "children": [
        {
          "type": "text",
          "content": "Thank You",
          "style": {
            "font-size": 80, "color": "#FFFFFF", "font-weight": "bold", "text-align": "center",
            "animation": [
              { "name": "scale_in", "duration": 0.8 },
              { "name": "wiggle", "property": "translate_y", "amplitude": 4, "frequency": 1.5, "seed": 7 }
            ]
          }
        },
        {
          "type": "icon",
          "icon": "lucide:heart",
          "size": { "width": 64, "height": 64 },
          "style": {
            "color": "#F43F5E",
            "animation": [
              { "name": "fade_in", "delay": 0.5 },
              { "name": "wiggle", "property": "scale", "amplitude": 0.1, "frequency": 2, "seed": 42 }
            ]
          }
        }
      ]
    }
  ]
}
```

---

## Reference

### JSON Scenario Structure

```json
{
  "version": "1.0",
  "video": { ... },
  "audio": [ ... ],
  "scenes": [ ... ]
}
```

#### `video` (required)

| Field        | Type   | Default     | Description                                             |
| ------------ | ------ | ----------- | ------------------------------------------------------- |
| `width`      | u32    | required    | Video width in pixels. **Must be even for H.264.**      |
| `height`     | u32    | required    | Video height in pixels. **Must be even for H.264.**     |
| `fps`        | u32    | `30`        | Frames per second                                       |
| `background` | string | `"#000000"` | Default background color (hex `#RRGGBB` or `#RRGGBBAA`) |
| `codec`      | string | `null`      | `"h264"` (10-bit), `"h265"`, `"vp9"`, `"prores"`        |
| `crf`        | u8     | `23`        | Constant Rate Factor (0-51, lower = better quality)     |

> **Encoding note:** H.264 outputs 10-bit (`yuv420p10le`) by default when ffmpeg is available, which reduces color banding on dark gradients. For best quality on gradient-heavy videos, use `--codec prores` (lossless).

#### `audio` (optional array)

| Field      | Type   | Default  | Description                                   |
| ---------- | ------ | -------- | --------------------------------------------- |
| `src`      | string | required | Path to audio file (wav, mp3, ogg, flac, aac) |
| `start`    | f64    | `0`      | Start time in seconds                         |
| `end`      | f64    | `null`   | End time (null = full duration)               |
| `volume`   | f32    | `1.0`    | Volume multiplier                             |
| `fade_in`  | f64    | `null`   | Fade in duration in seconds                   |
| `fade_out` | f64    | `null`   | Fade out duration in seconds                  |

#### `scenes` (required array)

| Field        | Type   | Default  | Description                                    |
| ------------ | ------ | -------- | ---------------------------------------------- |
| `duration`   | f64    | required | Scene duration in seconds (must be > 0)        |
| `background` | string | `null`   | Override video background for this scene       |
| `children`   | array  | `[]`     | Components rendered in order (first = back)    |
| `layout`     | object | `null`   | Scene-level flex layout (see below)            |
| `transition` | object | `null`   | Transition to this scene from the previous one |
| `freeze_at`  | f64    | `null`   | Freeze the scene at this time (seconds)        |

Each scene is an **implicit flex container** at video dimensions. All children participate in flex flow. Children with `position` inside a `card` become absolute. Default direction: `column`.

**IMPORTANT:** Every scene SHOULD include `"layout": {"align_items": "center", "justify_content": "center"}` for centered composition. Without this, content aligns to the top-left corner.

**`layout` options:** `direction` (column/row), `gap`, `align_items` (start/center/end/stretch), `justify_content` (start/center/end/space_between/space_around/space_evenly), `padding`

#### Include (Composable Scenarios)

Scene entries can reference external scenario files to inject their scenes inline:

```json
{
  "scenes": [
    { "include": "shared/intro.json" },
    { "duration": 5.0, "children": [...] },
    { "include": "shared/credits.json", "scenes": [0, 2] }
  ]
}
```

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `include` | string | required | Path (relative to parent) or URL to a scenario JSON file |
| `scenes` | array of usize | `null` | Only include scenes at these 0-based indices |

- The included file's `video` config is ignored
- Audio tracks from included files are merged
- Includes can be nested (max depth: 8)

#### Transitions

```json
{ "type": "fade", "duration": 0.5 }
```

**13 types:** `fade`, `wipe_left`, `wipe_right`, `wipe_up`, `wipe_down`, `zoom_in`, `zoom_out`, `flip`, `clock_wipe`, `iris`, `slide`, `dissolve`, `none`

Default duration: `0.5` seconds.

---

### Component Types

All components are discriminated by `"type"`. Rendered in array order (first = bottom). See Rule 7.

#### Common Optional Fields (root level)

| Field      | Type | Default | Description                                    |
| ---------- | ---- | ------- | ---------------------------------------------- |
| `start_at` | f64  | `null`  | Show component starting at this time (seconds) |
| `end_at`   | f64  | `null`  | Hide component after this time (seconds)       |

#### Common Style Fields (inside `"style"`)

| Style field | Type | Default | Description |
| --- | --- | --- | --- |
| `opacity` | f32 | `1.0` | 0.0 to 1.0 |
| `padding` | f32 or {top,right,bottom,left} | `null` | Inner spacing |
| `margin` | f32 or {top,right,bottom,left} | `null` | Outer spacing |
| `animation` | array or object | `[]` | Animation effects array (see below) |

#### Animation Style (inside `"style"`)

`style.animation` is a **typed array** of animation effects, each discriminated by `"name"`. A single effect (without array) is also accepted.

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

**Effect types:**

| Effect name | Fields | Description |
| --- | --- | --- |
| *preset name* | `delay`, `duration`, `loop`, `overshoot` | Any of the 39 presets (e.g. `fade_in_up`, `scale_in`) |
| *char preset* | `delay`, `duration`, `stagger`, `granularity`, `easing`, `overshoot` | Per-char/word animation: `char_scale_in`, `char_fade_in`, `char_wave`, `char_bounce`, `char_rotate_in`, `char_slide_up` |
| `glow` | `color`, `radius`, `intensity` | Luminous halo effect |
| `wiggle` | `property`, `amplitude`, `frequency`, `mode`, `seed`, ... | Procedural noise animation |
| `orbit` | `radius_x`, `radius_y`, `speed`, `depth`, `tilt`, ... | Elliptical/circular orbital motion with pseudo-3D depth |
| `keyframes` | `keyframes`, `delay`, `duration` | Custom keyframe animations |
| `motion_blur` | `intensity` | Motion blur effect |

**Preset fields:**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `delay` | f64 | `0` | Delay before animation starts (seconds) |
| `duration` | f64 | `0.8` | Animation duration (seconds) |
| `loop` | bool | `false` | Loop the animation continuously |
| `overshoot` | f64 | `0.08` | Overshoot/anticipation intensity for `scale_in`/`scale_out` (0.0 = none) |

**Glow fields:**

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `color` | string | `"#FFFFFF"` | Glow color (hex) |
| `radius` | f32 | `10.0` | Blur radius |
| `intensity` | f32 | `1.0` | Brightness multiplier |

---

### 1. `text`

```json
{
  "type": "text",
  "content": "Hello World",
  "max_width": 800,
  "style": {
    "font-size": 48,
    "color": "#FFFFFF",
    "font-family": "Arial",
    "font-weight": "bold",
    "text-align": "center",
    "line-height": 1.2,
    "letter-spacing": 2.0
  }
}
```

**Root fields:** `content` (required), `max_width`

| Style field       | Type     | Default    |
| ----------------- | -------- | ---------- |
| `font-size`       | f32      | `48.0`     |
| `color`           | string   | `"#FFFFFF"` |
| `font-family`     | string   | `"Inter"`  |
| `font-weight`     | enum     | `"normal"` — `"normal"`, `"bold"` |
| `font-style`      | enum     | `"normal"` — `"normal"`, `"italic"`, `"oblique"` |
| `text-align`      | enum     | `"left"` — `"left"`, `"center"`, `"right"` |
| `line-height`     | f32      | `null`     |
| `letter-spacing`  | f32      | `null`     |
| `text-shadow`     | object   | `null` — `{ "color": "#000", "offset_x": 2, "offset_y": 2, "blur": 4 }` |
| `stroke`          | object   | `null` — `{ "color": "#000", "width": 2 }` |
| `text-background` | object   | `null` — `{ "color": "#000", "padding": 4, "corner_radius": 4 }` |

**Per-character / per-word animation (char animation presets):**

Animates each character or word independently with staggered timing. Use `char_*` animation presets inside `style.animation`:

```json
{
  "type": "text",
  "content": "Hello World",
  "style": {
    "font-size": 64, "color": "#FFFFFF",
    "animation": [{ "name": "char_scale_in", "stagger": 0.03, "duration": 0.4, "delay": 0.2, "easing": "ease_out" }]
  }
}
```

**Char animation presets:** `char_scale_in`, `char_fade_in`, `char_wave`, `char_bounce`, `char_rotate_in`, `char_slide_up`

| Field         | Type   | Default    | Description                                      |
| ------------- | ------ | ---------- | ------------------------------------------------ |
| `stagger`     | f64    | `0.03`     | Delay between each unit (seconds)                |
| `duration`    | f64    | `0.4`      | Duration of each unit's animation (seconds)      |
| `delay`       | f64    | `0.0`      | Initial delay before the first unit starts       |
| `easing`      | string | `"linear"` | Easing function (same as keyframe easings)       |
| `granularity` | enum   | `"char"`   | `"char"` (per-character) or `"word"` (per-word)  |
| `overshoot`   | f64    | `0.08`     | Overshoot intensity for `char_scale_in`/`char_bounce` (0.0 = none) |

**Per-word mode** (`"granularity": "word"`) splits text by whitespace and animates each word as a unit. Ideal for headline reveals with larger stagger values (0.1-0.3s):

```json
{
  "type": "text",
  "content": "One platform to rule them all",
  "style": {
    "font-size": 56, "color": "#FFFFFF", "font-weight": "bold",
    "animation": [{ "name": "char_fade_in", "stagger": 0.15, "duration": 0.5, "granularity": "word" }]
  }
}
```

### 2. `shape`

```json
{
  "type": "shape",
  "shape": "rounded_rect",
  "size": { "width": 200, "height": 100 },
  "style": {
    "fill": "#FF5733",
    "border-radius": 16,
    "stroke": { "color": "#FFFFFF", "width": 2 }
  }
}
```

**Root fields:** `shape` (required), `size`, `text`

| Style field     | Type               | Default     |
| --------------- | ------------------ | ----------- |
| `fill`          | string or gradient | `null`      |
| `stroke`        | `{color, width}`   | `null`      |
| `border-radius` | f32                | `null`      |

**Shape types:** `rect`, `circle`, `rounded_rect`, `ellipse`, `triangle`, `star` (with `points`, default 5), `polygon` (with `sides`, default 6), `path` (with `data` SVG path string)

**Gradient fill:**
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

Types: `linear`, `radial`.

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
    "vertical_align": "middle"
  }
}
```

`vertical_align`: `"top"`, `"middle"`, `"bottom"` (default: `"middle"`). See Rule 5.

### 3. `image`

```json
{
  "type": "image",
  "src": "path/to/image.png",
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
  "size": { "width": 200, "height": 200 }
}
```

| Field      | Type              | Default                                                     |
| ---------- | ----------------- | ----------------------------------------------------------- |
| `src`      | string            | `null` — path to SVG file (either `src` or `data` required) |
| `data`     | string            | `null` — inline SVG markup                                  |
| `position` | `{x, y}`          | `{0, 0}`                                                    |
| `size`     | `{width, height}` | `null`                                                      |

### 5. `icon`

Renders an icon from the **Iconify** open-source framework (200,000+ icons from 150+ sets). Icons are fetched from the Iconify API at render time. Browse all icons: https://icon-sets.iconify.design/

```json
{
  "type": "icon",
  "icon": "lucide:home",
  "size": { "width": 64, "height": 64 },
  "style": { "color": "#38bdf8" }
}
```

| Field      | Type              | Default                                                  |
| ---------- | ----------------- | -------------------------------------------------------- |
| `icon`     | string            | required — Iconify id `"prefix:name"` (e.g. `"lucide:home"`) |
| `position` | `{x, y}`          | `{0, 0}`                                                 |
| `size`     | `{width, height}` | `{24, 24}`                                               |

Style: `color` (default `"#FFFFFF"`)

Common prefixes: `lucide` (UI), `mdi` (Material), `heroicons`, `ph` (Phosphor), `tabler`, `simple-icons` (brand logos), `devicon` (dev tools)

### 6. `video`

```json
{
  "type": "video",
  "src": "path/to/video.mp4",
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

### 7. `gif`

```json
{
  "type": "gif",
  "src": "path/to/animation.gif",
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

### 8. `caption`

Timed word-by-word captions with active word highlighting.

```json
{
  "type": "caption",
  "words": [
    { "text": "Hello", "start": 0.0, "end": 0.5 },
    { "text": "World", "start": 0.5, "end": 1.0 }
  ],
  "mode": "highlight",
  "max_width": 900,
  "style": { "font-size": 48, "color": "#FFFFFF" }
}
```

| Field          | Type     | Default                                                                    |
| -------------- | -------- | -------------------------------------------------------------------------- |
| `words`        | array    | required — `[{ "text", "start", "end" }]`                                  |
| `position`     | `{x, y}` | `{0, 0}`                                                                   |
| `mode`         | enum     | `"default"` — `"default"`, `"highlight"`, `"karaoke"`, `"bounce"` |
| `active_color` | string   | `"#FFD700"`                                                                |
| `max_width`    | f32      | `null`                                                                     |

Style: `font-size` (48.0), `font-family`, `color` (#FFFFFF), `background`

### 9. `counter`

Animated number counter. See Rule 4: must be standalone.

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
  "style": { "font-size": 72, "color": "#FFFFFF", "font-weight": "bold", "text-align": "center" }
}
```

**Root fields:** `from`, `to`, `decimals`, `separator`, `prefix`, `suffix`, `easing`

**Easing options:** `linear`, `ease_in`, `ease_out`, `ease_in_out`, `ease_in_quad`, `ease_out_quad`, `ease_in_cubic`, `ease_out_cubic`, `ease_in_expo`, `ease_out_expo`, `spring`

Style: `font-size` (48.0), `color` (#FFFFFF), `font-family` (Inter), `font-weight`, `text-align`, `letter-spacing`, `text-shadow`, `stroke`

### 10. Absolute Positioning (via `card`)

To place children at fixed absolute coordinates, use a `card` with transparent background and explicit size. Each child uses `position: {x, y}` relative to the card's top-left. Children with `position` become absolute inside a `card`.

```json
{
  "type": "card",
  "size": { "width": 1920, "height": 1080 },
  "style": { "background": "#00000000", "padding": 0 },
  "children": [
    { "type": "shape", "shape": "rect", "position": { "x": 0, "y": 0 }, "size": { "width": 400, "height": 300 }, "style": { "fill": "#1E293B", "border-radius": 16 } },
    { "type": "icon", "icon": "lucide:phone-off", "position": { "x": 170, "y": 120 }, "size": { "width": 64, "height": 64 }, "style": { "color": "#FFFFFF" } }
  ]
}
```

### 11. `card` / `flex`

Visual container with CSS-like flex & grid layout. `flex` is an alias for `card`. See Rule 8.

Each dimension of `size` can be a number or `"auto"`.

**Flex example:**
```json
{
  "type": "card",
  "size": { "width": 800, "height": 100 },
  "style": { "flex-direction": "row", "gap": 16 },
  "children": [
    { "type": "shape", "shape": "rect", "size": { "width": 100, "height": 100 }, "style": { "fill": "#FF0000" } },
    { "type": "shape", "shape": "rect", "size": { "width": 100, "height": 100 }, "style": { "fill": "#00FF00", "flex-grow": 1 } },
    { "type": "shape", "shape": "rect", "size": { "width": 100, "height": 100 }, "style": { "fill": "#0000FF" } }
  ]
}
```

**Grid example (2x2):** Note: grid containers need explicit `height` (not `"auto"`) — see [rules/grid-card-height.md](rules/grid-card-height.md).
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

| Style field              | Type        | Default    |
| ------------------------ | ----------- | ---------- |
| `display`                | enum        | `"flex"` — `"flex"` or `"grid"` |
| `background`             | string      | `null`     |
| `border-radius`          | f32         | `12.0`     |
| `border`                 | object      | `null` — `{ "color": "#E5E7EB", "width": 1 }` |
| `box-shadow`             | object      | `null` — `{ "color": "#00000040", "offset_x": 0, "offset_y": 4, "blur": 12 }` |
| `padding`                | f32 or obj  | `null`     |
| `flex-direction`         | enum        | `"column"` — `"column"`, `"row"`, `"column_reverse"`, `"row_reverse"` |
| `flex-wrap`              | bool        | `false`    |
| `align-items`            | enum        | `"start"` — `"start"`, `"center"`, `"end"`, `"stretch"` |
| `justify-content`        | enum        | `"start"` — `"start"`, `"center"`, `"end"`, `"space_between"`, `"space_around"`, `"space_evenly"` |
| `gap`                    | f32         | `0`        |
| `grid-template-columns`  | array       | `null` — `[{"px": N}, {"fr": N}, "auto"]` |
| `grid-template-rows`     | array       | `null`     |

**Per-child layout properties** (in child `"style"`):
- `flex-grow` (f32) — default 0
- `flex-shrink` (f32) — default 1
- `flex-basis` (f32) — defaults to natural size
- `align-self` (enum) — `"start"`, `"center"`, `"end"`, `"stretch"`
- `grid-column` (object) — `{ "start": 1, "span": 2 }` (1-indexed)
- `grid-row` (object) — `{ "start": 1, "span": 2 }` (1-indexed)

`position` is only valid inside `positioned` containers. Card children are laid out using flex/grid style properties.

### 12. `codeblock`

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
      "cursor": { "enabled": true }
    }
  ]
}
```

**Root fields:** `code` (required), `language`, `theme`, `size`, `show_line_numbers`, `chrome`, `highlights`, `reveal`, `states`

| Style field     | Type   | Default              |
| --------------- | ------ | -------------------- |
| `font-family`   | string | `"JetBrains Mono"`   |
| `font-size`     | f32    | `14.0`               |
| `font-weight`   | enum   | `"normal"`           |
| `line-height`   | f32    | `1.5` (multiplier)   |
| `background`    | string | `null` (uses theme)  |
| `border-radius` | f32    | `12.0`               |
| `padding`       | f32 or obj | `16`             |

**Available themes (72):** `base16-ocean.dark`, `base16-ocean.light`, `base16-eighties.dark`, `base16-mocha.dark`, `InspiredGitHub`, `Solarized (dark)`, `Solarized (light)`, `catppuccin-latte`, `catppuccin-frappe`, `catppuccin-macchiato`, `catppuccin-mocha`, `andromeeda`, `aurora-x`, `ayu-dark`, `ayu-light`, `ayu-mirage`, `dark-plus`, `dracula`, `dracula-soft`, `everforest-dark`, `everforest-light`, `github-dark`, `github-dark-default`, `github-dark-dimmed`, `github-dark-high-contrast`, `github-light`, `github-light-default`, `github-light-high-contrast`, `gruvbox-dark-hard`, `gruvbox-dark-medium`, `gruvbox-dark-soft`, `gruvbox-light-hard`, `gruvbox-light-medium`, `gruvbox-light-soft`, `horizon`, `horizon-bright`, `houston`, `kanagawa-dragon`, `kanagawa-lotus`, `kanagawa-wave`, `laserwave`, `light-plus`, `material-theme`, `material-theme-darker`, `material-theme-lighter`, `material-theme-ocean`, `material-theme-palenight`, `min-dark`, `min-light`, `monokai`, `night-owl`, `night-owl-light`, `nord`, `one-dark-pro`, `one-light`, `plastic`, `poimandres`, `red`, `rose-pine`, `rose-pine-dawn`, `rose-pine-moon`, `slack-dark`, `slack-ochin`, `snazzy-light`, `solarized-dark`, `solarized-light`, `synthwave-84`, `tokyo-night`, `vesper`, `vitesse-black`, `vitesse-dark`, `vitesse-light`

### 13. `divider`

Visual separator line.

```json
{
  "type": "divider",
  "direction": "horizontal",
  "thickness": 2,
  "line_style": "solid",
  "style": { "color": "#4B5563" }
}
```

**Root fields:** `direction` (horizontal/vertical), `thickness` (default 2.0), `line_style` (solid/dashed/dotted), `length` (optional fixed length)

Style: `color` (default `"#FFFFFF"`)

### 14. `badge`

Compact pill-shaped label with optional icon.

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

**Root fields:** `text` (required), `icon` (Iconify id), `variant` (solid/outline), `badge_size` (sm/md/lg)

Style: `background` (default `"#3B82F6"`) — badge color, `font-size`, `font-family`

### 15. `avatar`

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

**Root fields:** `src` (required), `size` (diameter, default 64), `border_color`, `border_width`, `status` (online/offline/away/none), `status_color`

### 16. `callout`

Speech bubble with directional arrow.

```json
{
  "type": "callout",
  "text": "Hello!",
  "arrow_direction": "bottom",
  "arrow_size": 12,
  "style": { "background": "#333333", "color": "#FFFFFF", "border-radius": 8, "font-size": 16 }
}
```

**Root fields:** `text` (required), `arrow_direction` (top/bottom/left/right), `arrow_size` (default 12), `size`

Style: `background` (default `"#333333"`), `color` (default `"#FFFFFF"`), `border-radius` (default 8), `font-size` (default 16), `font-family`

### 17. `terminal`

Terminal window with colored lines and chrome.

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

**Root fields:** `lines` (required — `[{ "text", "line_type", "color" }]`), `theme` (dark/light), `title`, `show_chrome` (default true), `reveal`, `size`

**Reveal:** `{ "mode": "typewriter"|"line_by_line", "start": 0, "duration": 1.0, "easing": "linear" }` — animates line/word appearance like the codeblock component.

Line types: `"prompt"` ($ prefix in green), `"command"` (white), `"output"` (gray)

Style: `font-size` (default 14)

### 18. `table`

Data table with headers and styled rows.

```json
{
  "type": "table",
  "headers": ["Name", "Role"],
  "rows": [["Alice", "Engineer"], ["Bob", "Designer"]],
  "size": { "width": 600, "height": 200 },
  "style": { "color": "#FFFFFF", "font-size": 14 }
}
```

**Root fields:** `headers` (required), `rows` (required), `header_color` (#374151), `row_colors` (alternating), `border_color` (#4B5563), `header_text_color`, `size`

Style: `color` (default `"#FFFFFF"`) — cell text color, `font-size` (default 14), `font-family`, `border-radius`

### 19. `chart`

Data visualization (bar/line/pie) with animation.

```json
{
  "type": "chart",
  "chart_type": "bar",
  "data": [
    { "value": 85, "label": "Q1" },
    { "value": 120, "label": "Q2" }
  ],
  "size": { "width": 400, "height": 300 }
}
```

**Root fields:** `chart_type` (required — bar/line/pie), `data` (required — `[{ "value", "label"?, "color"? }]`), `size` (default 300x200), `animated` (default true), `animation_duration` (default 1.5s), `colors` (custom palette)

Default palette: `#3B82F6`, `#EF4444`, `#22C55E`, `#F59E0B`, `#8B5CF6`, `#EC4899`, `#06B6D4`, `#F97316`

### 20. `mockup`

Device frame with image content inside.

```json
{
  "type": "mockup",
  "device": "iphone",
  "src": "screenshot.png",
  "theme": "dark"
}
```

**Root fields:** `device` (required — iphone/android/laptop/browser), `src` (required — path to image), `theme` (dark/light), `size`

Default sizes: iPhone 375x812, Android 360x800, Laptop 800x550, Browser 800x600

### 21. `particle`

Animated particle system for visual effects.

```json
{
  "type": "particle",
  "particle_type": "confetti",
  "count": 80,
  "speed": 1.2,
  "seed": 42
}
```

**Root fields:** `particle_type` (required — confetti/snow/stars/bubbles/halo), `count` (default 50), `colors`, `speed` (default 1.0), `size_range` ({min, max}, default {4, 12}), `seed` (default 42)

Behaviors: confetti=falling rotating rects, snow=falling circles, stars=twinkling fixed positions, bubbles=rising circles, halo=soft glowing circles drifting with pulsing opacity (use larger size_range like {30, 80} and low count ~10-15)

### 22. `arrow`

Directional arrow with optional bezier curves. Supports `draw_in` / `stroke_reveal` animation presets.

```json
{
  "type": "arrow",
  "x1": 100, "y1": 300,
  "x2": 500, "y2": 300,
  "curve": 0.3,
  "width": 3,
  "color": "#58A6FF",
  "arrow_end": true,
  "style": {
    "animation": [{ "name": "draw_in", "duration": 1.0 }]
  }
}
```

| Field         | Type          | Default    | Description                                              |
| ------------- | ------------- | ---------- | -------------------------------------------------------- |
| `x1`          | f32           | `0.0`      | Start X coordinate                                       |
| `y1`          | f32           | `0.0`      | Start Y coordinate                                       |
| `x2`          | f32           | required   | End X coordinate                                         |
| `y2`          | f32           | required   | End Y coordinate                                         |
| `cp`          | `{x, y}`     | `null`     | Quadratic bezier control point                           |
| `cp1`         | `{x, y}`     | `null`     | Cubic bezier first control point                         |
| `cp2`         | `{x, y}`     | `null`     | Cubic bezier second control point                        |
| `curve`       | f32           | `null`     | Auto-generate curve (-1.0 to 1.0, positive = up)         |
| `width`       | f32           | `3.0`      | Stroke width                                             |
| `color`       | string        | `"#FFFFFF"`| Arrow color (hex)                                        |
| `arrow_end`   | bool          | `true`     | Show arrowhead at end                                    |
| `arrow_start` | bool          | `false`    | Show arrowhead at start                                  |
| `arrow_size`  | f32           | `12.0`     | Arrowhead size                                           |
| `dashed`      | array of f32  | `null`     | Dash pattern (e.g. `[8, 4]`)                             |

### 23. `connector`

Connects two points with automatic routing (straight, curved, or elbow). Useful for diagrams and flowcharts.

```json
{
  "type": "connector",
  "from": { "x": 200, "y": 150 },
  "to": { "x": 600, "y": 400 },
  "routing": "curved",
  "curvature": 0.4,
  "color": "#58A6FF",
  "arrow_end": true,
  "style": {
    "animation": [{ "name": "stroke_reveal", "duration": 0.8 }]
  }
}
```

| Field         | Type          | Default      | Description                                          |
| ------------- | ------------- | ------------ | ---------------------------------------------------- |
| `from`        | `{x, y}`     | required     | Start point coordinates                              |
| `to`          | `{x, y}`     | required     | End point coordinates                                |
| `routing`     | enum          | `"straight"` | `"straight"`, `"curved"`, `"elbow"` (L-shaped path)  |
| `curvature`   | f32           | `0.4`        | Curve intensity (for `curved` routing)               |
| `width`       | f32           | `2.0`        | Stroke width                                         |
| `color`       | string        | `"#FFFFFF"`  | Line color (hex)                                     |
| `arrow_end`   | bool          | `true`       | Show arrowhead at end                                |
| `arrow_start` | bool          | `false`      | Show arrowhead at start                              |
| `arrow_size`  | f32           | `10.0`       | Arrowhead size                                       |
| `dashed`      | array of f32  | `null`       | Dash pattern (e.g. `[6, 3]`)                         |

### 24. `timeline`

Step-by-step timeline with animated progress bar, node icons, and labels.

```json
{
  "type": "timeline",
  "width": 800,
  "direction": "horizontal",
  "fill_progress": 0.75,
  "bar_fill_color": "#58A6FF",
  "steps": [
    { "label": "Design", "sublabel": "Week 1", "color": "#58A6FF", "icon": "1" },
    { "label": "Build", "sublabel": "Week 2-3", "color": "#58A6FF", "icon": "2" },
    { "label": "Test", "sublabel": "Week 4", "color": "#58A6FF", "icon": "3" },
    { "label": "Ship", "sublabel": "Week 5", "color": "#22C55E", "icon": "🚀" }
  ]
}
```

| Field            | Type   | Default      | Description                                         |
| ---------------- | ------ | ------------ | --------------------------------------------------- |
| `steps`          | array  | required     | `[{ "label", "sublabel"?, "color"?, "icon"? }]`     |
| `width`          | f32    | `800.0`      | Total timeline width                                 |
| `direction`      | enum   | `"horizontal"` | `"horizontal"` or `"vertical"`                    |
| `node_radius`    | f32    | `24.0`       | Radius of step circles                               |
| `bar_color`      | string | `"#333333"`  | Background bar color                                 |
| `bar_fill_color` | string | `"#58A6FF"`  | Filled bar color                                     |
| `bar_height`     | f32    | `4.0`        | Bar thickness                                        |
| `fill_progress`  | f32    | `1.0`        | Progress from 0.0 to 1.0 (animatable)               |
| `font_size`      | f32    | `16.0`       | Label font size                                      |
| `label_color`    | string | `"#FFFFFF"`  | Label text color                                     |
| `sublabel_color` | string | `"#8B949E"`  | Sublabel text color                                  |

**Step fields:**

| Field      | Type   | Default     | Description                          |
| ---------- | ------ | ----------- | ------------------------------------ |
| `label`    | string | required    | Step label text                      |
| `sublabel` | string | `null`      | Secondary label below/right of label |
| `color`    | string | `"#58A6FF"` | Node fill color when active          |
| `icon`     | string | `null`      | Emoji or single character in node    |

### 25. `lottie`

Renders Lottie animations from pre-rendered PNG frame sequences. Requires frames to be pre-generated externally.

```json
{
  "type": "lottie",
  "src": "animation.json",
  "frames_dir": "/path/to/frames",
  "size": { "width": 300, "height": 300 },
  "speed": 1.0,
  "loop": true
}
```

| Field       | Type              | Default  | Description                                                  |
| ----------- | ----------------- | -------- | ------------------------------------------------------------ |
| `src`       | string            | `null`   | Path to Lottie JSON file (for metadata: fps, frame count)    |
| `data`      | string            | `null`   | Inline Lottie JSON data (alternative to `src`)               |
| `frames_dir`| string            | `null`   | Directory with pre-rendered frames (`0000.png`, `0001.png`, ...) |
| `size`      | `{width, height}` | `null`   | Display size (falls back to Lottie intrinsic size)           |
| `speed`     | f32               | `1.0`    | Playback speed multiplier                                    |
| `loop`      | bool              | `true`   | Loop the animation                                           |

**Generating frames:** Use tools like `npx lottie-to-frames animation.json --output frames/` or puppeteer/lottie-web to pre-render Lottie frames as numbered PNGs.

### 26. `cursor`

Animated cursor with click effects, blinking, and path animation between waypoints.

```json
{
  "type": "cursor",
  "cursor_style": "default",
  "color": "#FFFFFF",
  "blink": 0.5,
  "click_at": [1.0, 2.5],
  "position": { "x": 400, "y": 300 }
}
```

**With auto-path (smooth movement between waypoints):**
```json
{
  "type": "cursor",
  "cursor_style": "default",
  "auto_path": [
    { "time": 0.5, "x": 100, "y": 200 },
    { "time": 1.5, "x": 400, "y": 300 },
    { "time": 2.5, "x": 600, "y": 150 }
  ],
  "path_easing": "ease_in_out",
  "click_duration": 0.3,
  "position": { "x": 200, "y": 200 }
}
```

| Field           | Type   | Default       | Description                                          |
| --------------- | ------ | ------------- | ---------------------------------------------------- |
| `width`         | f32    | `3.0`         | Cursor width                                         |
| `height`        | f32    | `40.0`        | Cursor height                                        |
| `color`         | string | `"#FFFFFF"`   | Cursor color                                         |
| `blink`         | f32    | `0.5`         | Blink cycle duration (0 = no blink)                  |
| `radius`        | f32    | `1.5`         | Corner radius                                        |
| `click_at`      | array  | `[]`          | Times to trigger click animation (seconds)           |
| `auto_path`     | array  | `[]`          | Waypoints: `[{ "time", "x", "y" }]`                 |
| `click_duration`| f32    | `0.3`         | Click animation duration                             |
| `cursor_style`  | string | `"default"`   | Cursor appearance style                              |
| `path_easing`   | string | `"ease_in_out"` | Path interpolation: `"linear"`, `"ease_out"`, `"ease_in_out"` |

**Notes:** When `auto_path` is set, click animations trigger automatically at each waypoint time. Cursor movement uses Catmull-Rom spline interpolation for smooth curves.

### 27. `line`

Simple line from (x1, y1) to (x2, y2). Supports `draw_in` / `stroke_reveal` animation.

```json
{
  "type": "line",
  "x1": 0, "y1": 0,
  "x2": 400, "y2": 200,
  "width": 2,
  "color": "#58A6FF",
  "style": {
    "animation": [{ "name": "draw_in", "duration": 0.8 }]
  }
}
```

| Field   | Type          | Default     | Description              |
| ------- | ------------- | ----------- | ------------------------ |
| `x1`    | f32           | `0.0`       | Start X                  |
| `y1`    | f32           | `0.0`       | Start Y                  |
| `x2`    | f32           | required    | End X                    |
| `y2`    | f32           | required    | End Y                    |
| `width` | f32           | `2.0`       | Stroke width             |
| `color` | string        | `"#FFFFFF"` | Line color               |
| `dashed`| array of f32  | `null`      | Dash pattern (e.g. `[8, 4]`) |

### 28. `rich_text`

Multi-styled text with individually styled spans on the same line. Inherits defaults from the component's `style`.

```json
{
  "type": "rich_text",
  "spans": [
    { "text": "Hello ", "color": "#FFFFFF", "font-weight": "bold" },
    { "text": "World", "color": "#58A6FF", "font-size": 64 }
  ],
  "max_width": 800,
  "style": { "font-size": 48, "color": "#FFFFFF" }
}
```

| Field       | Type   | Default | Description                              |
| ----------- | ------ | ------- | ---------------------------------------- |
| `spans`     | array  | required| `[{ "text", "color"?, "font-size"?, "font-weight"?, "font-family"?, "font-style"?, "letter-spacing"? }]` |
| `max_width` | f32    | `null`  | Maximum width before word-wrapping       |

**Span fields:** Each span inherits from the component's `style` for any unset field.

| Field           | Type   | Default     | Description          |
| --------------- | ------ | ----------- | -------------------- |
| `text`          | string | required    | Span text content    |
| `color`         | string | inherited   | Text color           |
| `font-size`     | f32    | inherited   | Font size            |
| `font-weight`   | enum   | inherited   | `"normal"` or `"bold"` |
| `font-family`   | string | inherited   | Font family          |
| `font-style`    | enum   | inherited   | `"normal"`, `"italic"`, `"oblique"` |
| `letter-spacing`| f32    | inherited   | Letter spacing       |

---

### Scene-Level Features

#### Virtual Camera

Scenes support a virtual camera with animatable pan, zoom, and rotation.

```json
{
  "duration": 5.0,
  "camera": {
    "x": 0, "y": 0, "zoom": 1.0, "rotation": 0,
    "keyframes": [
      { "property": "zoom", "values": [{ "time": 0, "value": 1.0 }, { "time": 3, "value": 1.5 }], "easing": "ease_in_out" },
      { "property": "x", "values": [{ "time": 0, "value": 0 }, { "time": 3, "value": -100 }], "easing": "ease_out" }
    ]
  },
  "children": [...]
}
```

| Field       | Type   | Default | Description                           |
| ----------- | ------ | ------- | ------------------------------------- |
| `x`         | f32    | `0.0`   | Camera center X offset (pixels)       |
| `y`         | f32    | `0.0`   | Camera center Y offset (pixels)       |
| `zoom`      | f32    | `1.0`   | Zoom factor (2.0 = 2x zoom in)       |
| `rotation`  | f32    | `0.0`   | Rotation in degrees                   |
| `keyframes` | array  | `[]`    | `[{ "property", "values": [{ "time", "value" }], "easing" }]` |

**Animatable properties:** `x`, `y`, `zoom`, `rotation`

#### Animated Background

Scenes can have animated gradient backgrounds. Gradients are interpolated in **linear color space** with subdivided color stops for smooth dark transitions. Use `concentric_circles` for a subtle, professional look (dark arc rings radiating from center). Use `gradient_shift` for color-shifting gradients.

```json
{
  "duration": 5.0,
  "animated-background": {
    "colors": ["#0F0E2A", "#1a1145", "#0F0E2A"],
    "speed": 15,
    "gradient_type": "radial",
    "preset": "concentric_circles",
    "element_size": 1.5,
    "count": 4
  },
  "children": [...]
}
```

| Field          | Type   | Default           | Description                                   |
| -------------- | ------ | ----------------- | --------------------------------------------- |
| `colors`       | array  | `[]`              | Gradient colors (hex)                         |
| `speed`        | f32    | `30.0`            | Animation speed (degrees/sec or pixels/sec)   |
| `gradient_type`| enum   | `"linear"`        | `"linear"` or `"radial"`                      |
| `preset`       | string | `null`            | `"gradient_shift"`, `"concentric_circles"`, `"grid_dots"` |
| `element_size` | f32    | `4.0`             | Dot/circle size for grid_dots; stroke width for concentric_circles |
| `spacing`      | f32    | `60.0`            | Element spacing for grid_dots/concentric_circles |
| `count`        | u32    | `null`            | Number of circles for concentric_circles (overrides spacing) |

---

### Additional Style Fields

New style fields available on all components:

| Style field       | Type   | Default | Description                                              |
| ----------------- | ------ | ------- | -------------------------------------------------------- |
| `backdrop-blur`   | f32    | `null`  | Glassmorphism blur effect (pixels)                       |
| `gradient-border` | object | `null`  | `{ "colors": [...], "width": 2, "angle": 0 }` — gradient-colored border |
| `inner-shadow`    | object | `null`  | `{ "color": "#000", "offset_x": 0, "offset_y": 0, "blur": 10 }` — inset shadow |
| `motion-path`     | string | `null`  | SVG path string that the element follows during animation |
| `stagger`         | f32    | `null`  | Auto-delay offset per child in a container (seconds)     |
| `timeline`        | array  | `[]`    | Intra-scene timeline steps — sequential animation phases |

---

### 3D Perspective Transforms

Any component can be rendered with true 3D perspective using keyframe animations on `rotate_x`, `rotate_y`, and `perspective` properties. The engine uses a Skia M44 4x4 matrix for real 3D rendering.

```json
{
  "type": "card",
  "position": { "x": 360, "y": 300 },
  "size": { "width": 1000, "height": 400 },
  "style": {
    "background": "#FFFFFF08",
    "border-radius": 24,
    "backdrop-blur": 15,
    "border": { "color": "#FFFFFF14", "width": 1 },
    "box-shadow": { "color": "#00000060", "offset_x": 0, "offset_y": 20, "blur": 60 },
    "animation": [{
      "name": "keyframes",
      "keyframes": [
        { "property": "rotate_x", "keyframes": [{ "time": 0, "value": 20 }, { "time": 2, "value": 8 }], "easing": "ease_out" },
        { "property": "rotate_y", "keyframes": [{ "time": 0, "value": -15 }, { "time": 2, "value": -5 }], "easing": "ease_out" },
        { "property": "perspective", "keyframes": [{ "time": 0, "value": 800 }, { "time": 2, "value": 800 }], "easing": "linear" }
      ]
    }]
  },
  "children": [...]
}
```

**3D Adaptive Shadow:** When a component has 3D rotation and a `box-shadow`, the shadow automatically shifts and scales based on the tilt angle — creating a realistic ground-plane shadow that moves opposite to the rotation direction.

---

### Timeline Sequencing

The `timeline` field on any component's style allows defining sequential animation phases within a single scene. Each step triggers at a specific time and applies its own animation effects relative to that time.

```json
{
  "type": "card",
  "style": {
    "animation": [{ "name": "fade_in_up", "duration": 0.6 }],
    "timeline": [
      {
        "at": 2.0,
        "animation": [{ "name": "shake", "duration": 0.5 }]
      },
      {
        "at": 4.0,
        "animation": [{ "name": "fade_out", "duration": 0.8 }]
      }
    ]
  }
}
```

| Field       | Type   | Description                                              |
| ----------- | ------ | -------------------------------------------------------- |
| `at`        | f64    | Time in seconds when this step activates                 |
| `animation` | array  | Animation effects to apply (same format as `style.animation`) |

**How it works:**
- Base `style.animation` plays from the start (e.g. entrance)
- Each timeline step activates when scene time reaches `step.at`
- Step animations resolve with time relative to `step.at` (so a step at 2.0s with a 0.5s animation runs from 2.0-2.5s)
- Multiple steps can overlap — their effects merge additively

**Use cases:** fade in → shake → fade out, entrance → highlight → exit, staged multi-phase animations without needing separate scenes.

---

### Animations

#### Custom Keyframe Animations

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

**Animatable properties:** `opacity`, `translate_x`, `translate_y`, `scale_x`, `scale_y`, `scale` (both axes), `rotation`, `blur`, `color`, `rotate_x`, `rotate_y`, `perspective`

**3D keyframe properties:**
- `rotate_x` — Rotation around X axis in degrees (tilts forward/backward)
- `rotate_y` — Rotation around Y axis in degrees (tilts left/right)
- `perspective` — Perspective distance in pixels (lower = more dramatic, typical: 800)

When any 3D property is animated, the component renders with a true 3D perspective transform (Skia M44 matrix). **3D adaptive shadows** are automatically computed — box-shadows shift and scale based on the rotation angles, creating a realistic ground-plane shadow effect.

**11 easing functions:** `linear`, `ease_in`, `ease_out`, `ease_in_out`, `ease_in_quad`, `ease_out_quad`, `ease_in_cubic`, `ease_out_cubic`, `ease_in_expo`, `ease_out_expo`, `spring`

**Spring physics** (when easing is `spring`):
```json
{
  "easing": "spring",
  "spring": { "damping": 15, "stiffness": 100, "mass": 1 }
}
```

#### Animation Presets

See Rule 13 for usage guidance.

```json
{
  "style": {
    "animation": [{ "name": "fade_in_up", "delay": 0.2, "duration": 0.8, "loop": false }]
  }
}
```

**39 presets:**

| Category   | Presets                                                                                                                                                                                                    |
| ---------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Entrances  | `fade_in`, `fade_in_up`, `fade_in_down`, `fade_in_left`, `fade_in_right`, `slide_in_left`, `slide_in_right`, `slide_in_up`, `slide_in_down`, `scale_in`, `bounce_in`, `blur_in`, `rotate_in`, `elastic_in` |
| Exits      | `fade_out`, `fade_out_up`, `fade_out_down`, `slide_out_left`, `slide_out_right`, `slide_out_up`, `slide_out_down`, `scale_out`, `bounce_out`, `blur_out`, `rotate_out`                                     |
| Continuous | `pulse`, `float`, `shake`, `spin` (use `"loop": true` in animation config — see Rule 9), `float_3d` (floating + 3D rotation, use `"loop": true`)                                                          |
| 3D         | `flip_in_x`, `flip_in_y`, `flip_out_x`, `flip_out_y` (3D card flip), `tilt_in` (3D tilt with rotate_x + rotate_y)                                                                                         |
| Stroke     | `draw_in` (animate `draw_progress` 0→1 for arrows/connectors/lines), `stroke_reveal` (draw_in + fade-in opacity over first 20%)                                                                            |
| Special    | `typewriter`, `wipe_left`, `wipe_right`                                                                                                                                                                    |
| Char (text only) | `char_scale_in`, `char_fade_in`, `char_wave`, `char_bounce`, `char_rotate_in`, `char_slide_up` (per-char/word animation, extra fields: `stagger`, `granularity`, `overshoot`) |

#### Wiggle (Procedural Noise)

See Rule 12 for combining with presets. Wiggle is an animation effect with `"name": "wiggle"` in the `style.animation` array.

```json
{
  "style": {
    "animation": [
      { "name": "wiggle", "property": "translate_x", "amplitude": 5, "frequency": 3, "seed": 42 },
      { "name": "wiggle", "property": "rotation", "amplitude": 8, "frequency": 4, "seed": 13, "decay": 0.6 }
    ]
  }
}
```

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `property` | string | required | Property to wiggle (same as animatable properties) |
| `amplitude` | f64 | required | Maximum deviation (pixels for translate, degrees for rotation) |
| `frequency` | f64 | required | Cycles per second (Hz). 0.8 = gentle float, 3 = wobble, 90 = vibration |
| `mode` | string | `"noise"` | `"noise"` (layered simplex) or `"sine"` (pure sine wave) |
| `seed` | u64 | `0` | Random seed for reproducible results (noise mode only) |
| `octaves` | u32 | `3` | Noise complexity (noise mode only) |
| `phase` | f64 | `0.0` | Phase offset |
| `decay` | f64 | `null` | Exponential decay rate |
| `easing` | string | `null` | Remap noise through an easing curve |

Wiggle offsets are applied **additively** on top of keyframe animations and presets.

#### Orbit (Circular/Elliptical Motion)

Orbit creates continuous circular or elliptical motion with pseudo-3D depth simulation. Like wiggle, it is **additive** on top of other animations.

```json
{
  "style": {
    "animation": [
      {
        "name": "orbit",
        "radius_x": 30,
        "radius_y": 20,
        "speed": 0.5,
        "depth": 0.15,
        "tilt": 20,
        "phase": 0.0
      }
    ]
  }
}
```

| Field           | Type | Default | Description                                               |
| --------------- | ---- | ------- | --------------------------------------------------------- |
| `radius_x`      | f64  | `30.0`  | Horizontal orbit radius (pixels)                          |
| `radius_y`      | f64  | `30.0`  | Vertical orbit radius (pixels)                            |
| `speed`          | f64  | `0.5`   | Revolutions per second                                    |
| `start_angle`    | f64  | `0.0`   | Starting angle in degrees (0=right, 90=bottom)            |
| `depth`          | f64  | `0.15`  | Scale modulation for pseudo-3D (0.0 = no depth, 1.0 = full) |
| `opacity_depth`  | f64  | `0.0`   | Opacity modulation for depth effect                       |
| `tilt`           | f64  | `0.0`   | Tilt angle of orbit plane in degrees                      |
| `phase`          | f64  | `0.0`   | Phase offset (0.0 to 1.0, shifts starting position)       |

**Use case:** Multiple elements orbiting with different `phase` values create a carousel effect.

---

### CLI Commands

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

### Pre-Delivery Checklist

Before presenting a generated scenario to the user, verify:

- [ ] **ZERO `rgba()` / `rgb()` values** — all colors in `#RRGGBB` or `#RRGGBBAA` hex format
- [ ] All scenes have `"layout": {"align_items": "center", "justify_content": "center"}` for centered composition
- [ ] `concentric_circles` animated-background on at least 4 scenes for visual depth
- [ ] No `end_at` on counters (makes them disappear — use `start_at` only)
- [ ] `rustmotion validate scenario.json` passes before presenting
