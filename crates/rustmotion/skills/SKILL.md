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
rustmotion validate -f scenario.json                # Validate (schema + geometry)
rustmotion validate -f scenario.json --fix          # Auto-fix safe overflows
rustmotion validate -f scenario.json --report r.json  # JSON report
rustmotion render -f scenario.json -o out.mp4       # Render to MP4
rustmotion render -f scenario.json -o f.png --frame 0  # Single frame
rustmotion schema                                   # Print JSON Schema
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

## Mental Model: Think HTML/CSS, not canvas

Rustmotion's JSON API is a direct superset of HTML/CSS. When composing a scene, **think "how would I write this in HTML/CSS?" first** — then translate. Do not think in terms of pixel coordinates; think in terms of flow, flex, and grid.

| HTML/CSS | Rustmotion JSON |
|---|---|
| `<body style="display:flex;flex-direction:column;align-items:center;justify-content:center">` | `"layout": {"direction": "column", "align_items": "center", "justify_content": "center"}` |
| `<div>` neutre — layout pur, zéro décoration visuelle | `{"type":"div"}` — flex par défaut, pas de fond/border-radius/ombre |
| `<div class="card">` — avec fond, border-radius, ombre | `{"type":"card"}` — flex par défaut, styling visuel |
| `<div style="display:flex;flex-direction:row;gap:24px">` | `{"type":"div","style":{"flex-direction":"row","gap":24}}` |
| `<div style="display:grid;grid-template-columns:1fr 1fr;gap:16px">` | `{"type":"div","style":{"display":"grid","grid-template-columns":["1fr","1fr"],"gap":16}}` |
| `<h1>Title</h1>` — inline, no position | `{"type":"text","content":"Title"}` — flow child, no `x`/`y` |
| `<div style="position:absolute;top:400px;left:0;width:100%;height:100%">` | `{"position":"absolute","x":0,"y":400,"style":{"width":1080,"height":1920}}` |
| `margin`, `padding`, `gap` | same names — spacing between and around elements |

### Flow vs Absolute — la règle d'or

```
Normal flow (default)       → title, cards, icons, text, buttons, grids
position: "absolute"        → background blobs, particle layers, floating badge overlays
```

Children without `position` participate in the flex/grid flow and are centered automatically by the parent layout. Children with `position: "absolute"` are removed from the flow and placed at exact `x`/`y` coordinates.

### Positionnement relatif : choisir le bon outil

Tout espace, alignement, et distribution se règle via des propriétés sur le **parent** — jamais via `x`/`y` sur les enfants.

| Besoin | Propriété | Sur qui |
|---|---|---|
| Espace entre enfants | `gap` | Parent flex/grid |
| Espace entre contenu et bordure | `padding` | Le container |
| Centrer horizontalement | `align-items: "center"` (column) ou `justify-content: "center"` (row) | Parent |
| Centrer verticalement | `justify-content: "center"` (column) | Parent |
| Élément prend tout l'espace restant | `flex-grow: 1` | L'enfant |
| Pousser un enfant à droite | `margin-left: "auto"` | Cet enfant |
| 2 colonnes égales | `display: grid` + `grid-template-columns: ["1fr","1fr"]` | Parent |
| Exception d'alignement pour 1 enfant | `align-self` | Cet enfant |

**Anti-pattern : penser en coordonnées**
```json
// ❌ — x/y partout, fragile, recalcul manuel à chaque changement
{ "type": "text", "content": "Titre", "position": "absolute", "x": 200, "y": 400 }
{ "type": "text", "content": "Sous-titre", "position": "absolute", "x": 200, "y": 530 }
{ "type": "card", "position": "absolute", "x": 90, "y": 700, "style": { "width": 900 } }
```

**Pattern correct : penser en HTML/CSS**
```json
// ✅ — gap + padding + flex, rien à calculer, s'adapte automatiquement
{ "layout": { "direction": "column", "align_items": "center", "justify_content": "center", "gap": 32 },
  "children": [
    { "type": "text", "content": "Titre", "style": { "font-size": 96, "text-align": "center" } },
    { "type": "text", "content": "Sous-titre", "style": { "font-size": 48 } },
    { "type": "card", "style": { "width": 900, "padding": 48, "gap": 24 }, "children": [...] }
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
4. **Dynamism level** — How much motion do you want post-entrance? (0) Static — elements enter then freeze; (1) Subtle — 1-2 gentle floats/wiggles; (2) Dynamic — floating hero, depth cards, camera zoom reveals; (3) Cinematic — camera pan/zoom, orbital backgrounds, multi-layer parallax. Default: 1. See [rules/dynamic-depth.md](rules/dynamic-depth.md). If level ≥ 2, also ask which parallax approach: (A) `float_3d` + wiggle seeds — per-element depth, (B) Camera keyframes — cinematic pan/zoom, (C) Orbital backgrounds — decorative ambient layer. Multiple choices combine well.
5. **Key content** — What text, data, features, or CTA should appear?
6. **Color palette** — Brand colors? If not, pick a tone: (A) Dark Tech — navy + indigo, (B) Corporate — white + blue, (C) Playful — dark + amber/pink, (D) Minimal — white + black. Exact hex values: see [rules/color-palettes.md](rules/color-palettes.md).

Skip questions where the answer is already obvious from context.

### Phase 2: Scene plan with component suggestions

Based on the brief, propose a **structured scene plan** using the table format below. This commits palette, sizes, and animation budgets before any JSON is written — so the user can catch mismatches early.

**Header block:**
```
## Plan vidéo — [titre]
Device: [Mobile 9:16 / Desktop 16:9 / Square] | Durée totale: [Xs] | Ton: [Corporate/Playful/…]
Palette: BG [#hex] | Texte [#hex] | Accent [#hex] | Cards [#hex]
  → See rules/color-palettes.md for the 4 ready-to-use palettes.
Dynamisme: [0 Static / 1 Subtle / 2 Dynamic / 3 Cinematic] — [chosen parallax approach]
  → See rules/dynamic-depth.md for patterns and recipes.
Style animations: [e.g. "fade_in_up entrances, stagger 0.2s, ease_out — no exit animations"]
```

**Scene table (one row per scene):**

| # | Durée | Nom | Composants | Tailles texte clés | Budget animation | Effets dynamiques |
|---|---|---|---|---|---|---|
| 1 | 3.5s | Intro hero | icon hero (180px) + text titre + animated-bg radial | titre: 108px bold | fade_in_up: 0+0.6 → 0.6s ✓ | float_3d loop, camera zoom 1.1→1.0 |
| 2 | 5.5s | Features | 3× card(row, stagger 0.2s) + icon feature (80px) + text body | body: 54px | stagger: 0+0.2+0.4 + 0.6 → 1.0s ✓ | wiggle seeds 7/42/91 per card |
| 3 | 3.0s | CTA | badge + text titre + glow | titre: 108px | fade_in_up 0.3+0.6 → 0.9s ✓ | float_3d loop, camera zoom 1.05→1.0 |

**Validation column "Budget animation":** compute `last_delay + last_duration` and mark ✓ if ≤ scene duration, ✗ if not. See [rules/animation-completion-budget.md](rules/animation-completion-budget.md).

Each scene must include:
- **Concrete components** (text, card, icon, shape, badge, counter, etc.) with explicit sizes for the target device
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
| Hero / intro | `text` with `char_scale_in` + main `icon` (hero role: 160-200px mobile / 80-100px desktop) |
| Transition / ambiance | `particle` stars/confetti + `animated-background` |
| Grouped transforms | `div` wrapping children + shared `timeline` scale/fade |

The user validates or adjusts the plan before proceeding.

### Phase 3: Iterative scene-by-scene construction

**Pre-generation checklist** — verify before writing each scene's JSON:
0. **HTML/CSS mental model** — sketch the layout mentally as HTML divs before writing JSON. Every element should have a reason to be in flow OR absolute. If you're reaching for `x`/`y` on a main content element, stop and restructure: use `gap` for spacing between siblings, `padding` for inner spacing, `align-items`/`justify-content` for centering, `flex-grow` for elastic elements. `x`/`y` is reserved for decorative blobs and overlays only. See [rules/html-css-mental-model.md](rules/html-css-mental-model.md).
1. All font sizes meet the floor for the target device (see [rules/typography-readability.md](rules/typography-readability.md))
2. `start_at + delay + duration ≤ scene_duration` for every animation (see [rules/animation-completion-budget.md](rules/animation-completion-budget.md))
3. Text color contrasts correctly with the scene/card background (dark bg → white text, light bg → dark text)
4. If a `counter` is inside a card (this is fine — it centers correctly), make sure the card/parent is at least as wide as the counter's worst-case digit width, since the counter box never shrinks to fit (see [rules/counter-standalone.md](rules/counter-standalone.md))
5. Scene duration ≥ reading time of all text (`word_count ÷ 2.5`) (see [rules/scene-pacing.md](rules/scene-pacing.md))
6. If dynamism level ≥ 2: at least one non-text element per scene has a continuous effect (`float_3d`/`wiggle`/`orbit` with `loop: true`). Never apply continuous motion to primary text. See [rules/dynamic-depth.md](rules/dynamic-depth.md).

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

- **Scene duration:** Use `max(animation_budget + 0.5s_dwell, word_count ÷ 2.5)`. Never use "3-5s" as a flat default — text-heavy scenes need more. See [rules/scene-pacing.md](rules/scene-pacing.md) for the lookup table.
- **Typography floor:** Title ≥ 90px (mobile) / 45px (desktop). Body ≥ 48px (mobile) / 24px (desktop). `line-height: 1.4–1.6` on multi-line **body copy**. White text on dark bg; dark text on light bg. See [rules/typography-readability.md](rules/typography-readability.md).
- **Statement register** (1600.agency/Machina, Aikido-style dark-premium): display type 200–400px+ on desktop against 30–56px labels, `line-height` 0.80–1.05 (tight, deliberate — not the body-copy floor above), `letter-spacing` **positive** for brutalist/uppercase or **negative** (e.g. `-2` uniform) for the dark-premium register. See [rules/typography-readability.md](rules/typography-readability.md#statement-register-hierarchy-scale-tracking) and [rules/1600-brutalist-style.md](rules/1600-brutalist-style.md).
- **Animation budget:** `start_at + delay + duration ≤ scene_duration` for every animated component. See [rules/animation-completion-budget.md](rules/animation-completion-budget.md).
- **Color palette:** Pick one of the 4 pre-built palettes (Dark Tech / Corporate / Playful / Minimal) in Phase 2 and never deviate. See [rules/color-palettes.md](rules/color-palettes.md).
- **Animation patterns:** Stagger entrances within a scene (0.1-0.3s delays). Use fade/slide transitions between scenes.
- **Backgrounds:** Radial gradient for dark themes, concentric_circles for tech feel, particles for ambiance.
- **Visual hierarchy:** Title (large font) → subtitle (medium) → body (smaller). Use color contrast to guide the eye.
- **Consistency:** Same color palette and animation style across all scenes.
- **Pacing:** Never place two dense scenes back-to-back — insert a breathing scene (1.5-2.5s) between them. See [rules/scene-pacing.md](rules/scene-pacing.md).
- **Icons:** Use the 3-role hierarchy (hero 160-200px / card 72-96px / inline 48-60px on mobile). See [rules/icon-sizing-hierarchy.md](rules/icon-sizing-hierarchy.md).
- **Device-aware sizing:** All component sizes MUST be scaled to the target device (×3 for mobile, ×1.5 for desktop, ×2.5 for square). A title on mobile = 108px, NOT 48px. See [rules/responsive-device-sizing.md](rules/responsive-device-sizing.md).

---

## Rules

Read individual rule files for detailed explanations, GOOD/BAD examples, and constraints:

- [rules/html-css-mental-model.md](rules/html-css-mental-model.md) - **CRITICAL:** Think HTML/CSS — flow layout first, absolute only for decorative/overlay elements
- [rules/validate-json.md](rules/validate-json.md) - Always validate generated JSON with `rustmotion validate` before presenting
- [rules/geometry-safety.md](rules/geometry-safety.md) - Keep all content inside the viewport: `white-space`, `auto_scroll`, `overflow` semantics + violation kinds
- [rules/even-dimensions.md](rules/even-dimensions.md) - Use even width/height for H.264 encoding
- [rules/counter-standalone.md](rules/counter-standalone.md) - Counter centers correctly in a card; size the parent for its worst-case digit width or it overflows silently
- [rules/vertical-align.md](rules/vertical-align.md) - Shape text vertical_align: use "top"/"middle"/"bottom" (NOT "center")
- [rules/stagger-animations.md](rules/stagger-animations.md) - Stagger animations with increasing style.animation.delay
- [rules/layer-order.md](rules/layer-order.md) - Layer order matters: first in array = behind, last = front
- [rules/card-flex-layout.md](rules/card-flex-layout.md) - Scene = implicit flex container; use card/flex for nested layout
- [rules/pixel-product-register.md](rules/pixel-product-register.md) - Complete visual register for demonstrating a CLI product: palette, the two type scales, the composed terminal pane, beat proportions, and which transition to spend where. Every value measured, with the derivations kept.
- [rules/world-view.md](rules/world-view.md) - **CRITICAL:** `world` view = the only mechanism for real continuity across beats (no scene-boundary cuts); `world-position` coordinate model + ambient-halo recipe
- [rules/continuous-presets.md](rules/continuous-presets.md) - Continuous presets (pulse, float, shake, spin) need loop: true
- [rules/timing-constraints.md](rules/timing-constraints.md) - Timing: start_at must be < end_at, duration > 0
- [rules/icon-format.md](rules/icon-format.md) - Icons use Iconify (200k+ icons), format "prefix:name" (e.g. "lucide:home")
- [rules/grid-card-height.md](rules/grid-card-height.md) - Grid containers need explicit height (not "auto") to prevent row stretching
- [rules/wiggle-additive.md](rules/wiggle-additive.md) - Wiggle is additive on top of presets and keyframes
- [rules/prefer-presets.md](rules/prefer-presets.md) - Prefer presets over manual keyframes (40 built-in presets + 6 char-only)
- [rules/hex-colors.md](rules/hex-colors.md) - Colors in hex format only (#RRGGBB or #RRGGBBAA)
- [rules/easing-guidelines.md](rules/easing-guidelines.md) - Easing guidelines for motion design
- [rules/text-background.md](rules/text-background.md) - text-background renders a colored rectangle behind text
- [rules/3d-perspective.md](rules/3d-perspective.md) - 3D perspective transforms with rotate_x, rotate_y, perspective keyframes
- [rules/timeline-sequencing.md](rules/timeline-sequencing.md) - Timeline steps for multi-phase animations within a single scene
- [rules/gradient-quality.md](rules/gradient-quality.md) - Gradient quality: linear color space, 10-bit encoding, ProRes for dark gradients
- [rules/video-wizard.md](rules/video-wizard.md) - Video creation wizard: iterative scene-by-scene construction best practices
- [rules/responsive-device-sizing.md](rules/responsive-device-sizing.md) - CRITICAL: Scale all sizes to target device using Tailwind 4 type scale (×3 mobile, ×1.5 desktop)
- [rules/chart-types.md](rules/chart-types.md) - Chart type selection guide (12 types: bar, line, area, donut, funnel, waterfall, radar, scatter, etc.)
- [rules/stat-cards.md](rules/stat-cards.md) - Stat/KPI cards best practices (trend, sparkline, dashboard layout)
- [rules/data-viz-components.md](rules/data-viz-components.md) - Data visualization component selection (gauge vs progress, sparkline vs chart, skeleton patterns)
- [rules/ui-controls.md](rules/ui-controls.md) - Switch, slider, rating: animated interactive control patterns
- [rules/notification-stacking.md](rules/notification-stacking.md) - Notification stacking: push_at, wait_for_push, variant colors
- [rules/dot-map-coordinates.md](rules/dot-map-coordinates.md) - Dot map: use real lat/lng coordinates, common city reference table

### Design quality (nouvelles règles)

- [rules/animation-completion-budget.md](rules/animation-completion-budget.md) - **CRITICAL:** Animation budget formula — every animation must complete within its scene duration
- [rules/typography-readability.md](rules/typography-readability.md) - **CRITICAL:** Minimum font sizes per device/role, line-height rules, contrast hard rules
- [rules/scene-pacing.md](rules/scene-pacing.md) - Scene duration formula (reading time + animation budget), density limits, dense/breathing alternation
- [rules/color-palettes.md](rules/color-palettes.md) - 4 ready-to-use palettes (Dark Tech / Corporate / Playful / Minimal), consistency rules
- [rules/icon-sizing-hierarchy.md](rules/icon-sizing-hierarchy.md) - Icon sizing (hero/card/inline roles), card spacing minimums, row layout by device
- [rules/depth-layering.md](rules/depth-layering.md) - **NEW:** Visual depth — 3 planes (bg/mid/fg), z-index, blur, shadow hierarchy, scale gradient, 3D tilt
- [rules/dynamic-depth.md](rules/dynamic-depth.md) - **NEW:** Multi-element parallax — wiggle seeds, float_3d preset, camera zoom, orbit phases, frequency hierarchy
- [rules/component-field-placement.md](rules/component-field-placement.md) - **CRITICAL:** Field placement (root vs style) — `width`/`height`/`animation` inside `style`; `fill`/`stroke`/`timeline`/`stagger` at root; `box-shadow` as array; silently-dropped component pitfalls
- [rules/badge-video-sizing.md](rules/badge-video-sizing.md) - Badge sizing for video resolution — `badge_size` sm/md/lg is too small at 1080px; use `style.font-size` to override (40px recommended for 1080×1920)
- [rules/glassmorphism.md](rules/glassmorphism.md) - Frosted-glass card recipe: `backdrop-filter: blur`, translucent background, subtle border, layered over a colorful background
- [rules/audio-reactive.md](rules/audio-reactive.md) - Bind `style.audio-reactive` to an `audio` track — drives `waveform`/`audio_spectrum` and reactive scale/opacity on any component
- [rules/captions-workflow.md](rules/captions-workflow.md) - Generating `caption` word timings from a transcript/audio track
- [rules/time-remapping.md](rules/time-remapping.md) - `time_scale`/`time_offset` on containers — slow-motion, freeze-frame, and time-shifted children

### Architecture (pour contribuer au code)

- [rules/paint-context.md](rules/paint-context.md) - Painter trait API: paint_content(canvas, layout, props, ctx) — remplace l'ancien Widget
- [rules/module-structure.md](rules/module-structure.md) - Structure des crates: rustmotion-core (css/, engine/, traits/) + rustmotion-components (57 composants)

---

## Complete Examples

The two examples below are short excerpts. For full, validated, end-to-end scenarios to study or copy from, see the `examples/` directory at the repo root — these are ahead of this prose (they use `fill`/`stroke`/`timeline`/`stagger` at root, correct `grid-template-columns` syntax, etc.) and are re-validated on every change:

| File | Resolution | Scenes | What it demonstrates |
|---|---|---|---|
| `examples/demo.json` | 1080×1920 | 6 | Minimal skeleton — just `video` + solid-color scenes |
| `examples/component-showcase.json` | 1920×1080 | 4 | Broad tour of basic + data-viz + UI components |
| `examples/dynamic-glass.json` | 1920×1080 | 3 | Glassmorphism, `backdrop-filter`, depth layering |
| `examples/rustmotion-promo.json` | 1920×1080 | 6 | Product promo pacing, stagger, char animations |
| `examples/ferriskey-presentation.json` | 1920×1080 | 6 | Slide-deck style presentation, heavy char/word stagger |
| `examples/mega-showcase.json` | 1920×1080 | 9 | Largest example — grid layout, timeline component, most component types in one file |

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
          "fill": {
            "type": "linear",
            "colors": ["#6366f1", "#8b5cf6"],
            "angle": 135
          },
          "style": {
            "width": 900,
            "height": 520,
            "border-radius": 32,
            "animation": [{ "name": "scale_in", "duration": 0.6 }]
          }
        },
        {
          "type": "icon",
          "icon": "lucide:rocket",
          "style": {
            "width": 80,
            "height": 80,
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
          "show_line_numbers": true,
          "chrome": { "enabled": true, "title": "src/main.rs" },
          "reveal": { "mode": "typewriter", "start": 0.5, "duration": 3.0 },
          "style": { "width": 1400, "height": 400, "font-size": 22, "padding": 24, "border-radius": 16 },
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
          "style": { "font-size": 96, "color": "#38BDF8", "font-weight": "bold", "text-align": "center" }
        },
        {
          "type": "text",
          "content": "Users Reached",
          "style": { "font-size": 36, "color": "#CBD5E1", "text-align": "center", "animation": [{ "name": "fade_in_up", "delay": 0.5, "duration": 0.6 }] }
        },
        {
          "type": "card",
          "style": {
            "width": 900,
            "height": "auto",
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
              "style": { "width": 48, "height": 48, "color": "#22C55E" }
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
          "style": {
            "width": 64,
            "height": 64,
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
| `world-position` | `{x, y}` | `null` | **(world view only)** Camera waypoint for this scene — NOT the scene's origin. See [rules/world-view.md](rules/world-view.md). |
| `persist`    | bool   | `false`  | **(world view only)** Keep this scene's content visible (fully opaque) after its own time window ends |
| `camera`     | object | `null`   | Virtual camera (pan/zoom/rotation) — see [Virtual Camera](#virtual-camera) below |

Note the casing: `world-position` is kebab-case, `freeze_at` is snake_case — a real inconsistency in the schema, not a typo. Copy the field name exactly as shown.

Each scene is an **implicit flex container** at video dimensions. All children participate in flex flow. Children with `position` inside a `card` become absolute. Default direction: `column`.

**IMPORTANT:** Every scene SHOULD include `"layout": {"align_items": "center", "justify_content": "center"}` for centered composition. Without this, content aligns to the top-left corner.

**`layout` options:** `direction` (column/row), `gap`, `align_items` (start/center/end/stretch), `justify_content` (start/center/end/space_between/space_around/space_evenly), `padding`

#### Layout Strategy: Prefer Flex/Grid — Absolute is a last resort

Think of scene composition exactly like HTML/CSS: **prefer normal flow** (flex column/row, gap, nested cards) over absolute positioning. The scene itself is a flex column; children stack naturally.

**Use flex/grid for:**
- Main content stacking (hero text + subtitle + CTA button)
- Side-by-side cards (`flex-direction: row`)
- Grid of cards (2×2, 3×1, etc.) — use `display: grid` inside a card
- Icon + text pairs inside a card

**Use `position: "absolute"` ONLY for:**
- Decorative background elements (ambient blobs, particles, shapes) that shouldn't affect flow
- Floating UI badges or tooltips that visually overlay content
- Elements that need to live at a precise pixel position regardless of surrounding content

**Anti-pattern to avoid:**
```json
// BAD — using absolute for everything like old-school CSS
{ "type": "text", "content": "Title", "position": "absolute", "x": 200, "y": 400 }

// GOOD — let it flow in the flex column
{ "type": "text", "content": "Title", "style": { "font-size": 84, "text-align": "center" } }
```

**Decorative/background shapes must always be absolute** — a non-absolute shape or particle in the flex flow consumes height and can push content off-center or to the bottom of the screen. Always add `"position": "absolute", "x": 0, "y": 0` to ambient shapes and particles.

#### Views & Composition (`composition`)

`scenes` at the scenario root is shorthand for a single implicit `slide` view. For multiple **views** — or to unlock the `world` view — use `composition` instead. `composition` and root-level `scenes` are mutually exclusive (using both is an error).

```json
{
  "version": "1.0",
  "video": { "width": 1920, "height": 1080, "fps": 30 },
  "composition": [
    { "type": "slide", "scenes": [ { "duration": 3.0, "children": [ { "type": "text", "content": "Slide beat" } ] } ] },
    {
      "type": "world",
      "camera_pan_duration": 1.0,
      "camera_easing": "ease_in_out",
      "background": { "preset": "halo", "zones": [ { "color": "#6366F1AA", "x": 0.3, "y": 0.4, "radius": 0.5 } ] },
      "scenes": [
        { "duration": 2.5, "children": [ { "type": "text", "content": "World beat one" } ] },
        { "duration": 2.5, "children": [ { "type": "text", "content": "World beat two" } ] }
      ]
    }
  ]
}
```

| Field | Type | Default | Description |
| --- | --- | --- | --- |
| `type` | enum | `"slide"` | `"slide"` (scene-to-scene, coupe/transition) or `"world"` (continuous virtual camera) |
| `scenes` | array | `[]` | Same scene objects as root `scenes` (supports `include` too) |
| `transition` | object | `null` | Transition **entering this view** from the previous view (same shape as a scene `transition`) |
| `background` | string/object | `null` | Shared background for the whole view — for `world`, this is where the ambient glow layer belongs (`preset: "halo"`), never as per-scene shapes |
| `camera_easing` | enum | `"ease_in_out"` | **(world)** Easing for the camera pan between scene waypoints |
| `camera_pan_duration` | f64 | `0.8` | **(world)** Duration (seconds) of the camera pan at each scene boundary |

**`slide` views** are what the rest of this document describes: scenes render in sequence, `transition` composites two already-rendered frame buffers (fade/wipe/zoom/…) — no element survives the cut.

**`world` views** are the only mechanism that produces real continuity between beats: a single virtual camera glides between scene waypoints (`world-position`), with a crossfade during the pan instead of a hard cut, over a shared `background`. Full recipe, coordinate model, and a validated multi-beat example: [rules/world-view.md](rules/world-view.md).

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
| `config` | object | `null` | Config overrides for structural components |

- The included file's `video` config is ignored
- Audio tracks from included files are merged
- Includes can be nested (max depth: 8)

#### Structural Components (Config)

Structural components are reusable scenarios with **declared config** (type + default). When rendered standalone, defaults apply. When included, the parent can override config values. Config supports all types including `array` and `object`, allowing full component trees (e.g. rich_text spans) to be passed as parameters.

**Defining a structural component (`components/outro.json`):**
```json
{
  "config": {
    "cta_text": { "type": "string", "default": "Book your demo" },
    "accent_color": { "type": "string", "default": "#5C39EE" },
    "logo_src": { "type": "string", "default": "assets/logo.svg" },
    "counter_target": { "type": "number", "default": 400 },
    "tagline_spans": {
      "type": "array",
      "default": [
        { "text": "Don't " },
        { "text": "miss ", "color": "#B041F0" },
        { "text": "any lead" }
      ]
    }
  },
  "video": { "width": 1080, "height": 1920, "fps": 30 },
  "scenes": [
    {
      "duration": 7.0,
      "children": [
        { "type": "svg", "src": "$logo_src" },
        { "type": "text", "content": "$cta_text", "style": { "color": "$accent_color" } },
        { "type": "counter", "from": 0, "to": { "$var": "counter_target" } },
        { "type": "rich_text", "spans": { "$var": "tagline_spans" } }
      ]
    }
  ]
}
```

**Config reference syntax:**

| Syntax | When to use | Behavior |
| --- | --- | --- |
| `"$name"` | Whole string value | Replaced by the config value (preserves type: number, boolean, etc.) |
| `"text $name text"` | String interpolation | Inline substitution (value must be string/number/boolean) |
| `{ "$var": "name" }` | Non-string in object position | Replaced by the config value (arrays, objects, numbers) |
| `"$$literal"` | Escape | Produces literal `"$literal"` |

**Including with overrides:**
```json
{
  "scenes": [
    { "duration": 5.0, "children": [...] },
    {
      "include": "components/outro.json",
      "config": {
        "cta_text": "Try WhatsApp",
        "accent_color": "#25D366",
        "tagline_spans": [
          { "text": "Stop losing " },
          { "text": "customers", "color": "#25D366" }
        ]
      }
    }
  ]
}
```

Config types: `string`, `number`, `boolean`, `object`, `array`. Omitted overrides use defaults. Referencing an undefined config key is an error.

**Rules for generation:**
- Always declare config entries with a `type` and `default`
- Use `"$name"` for string fields (src, content, color) — replaces the whole value
- Use `{ "$var": "name" }` for non-string fields (numbers, arrays, objects) to preserve the type
- Use `array` type to pass component trees (spans, children, gradient color stops)
- Use `$$` to escape literal dollar signs
- Never reference config values inside the `"config"` definition block itself

#### Transitions

```json
{ "type": "fade", "duration": 0.5 }
```

**15 types:** `fade`, `wipe_left`, `wipe_right`, `wipe_up`, `wipe_down`, `zoom_in`, `zoom_out`, `flip`, `clock_wipe`, `iris`, `slide`, `dissolve`, `corner_reveal`, `pixel_dissolve`, `none`

`corner_reveal` uncovers the incoming scene through a rectangle anchored at one
corner: two edges stay pinned to the frame, the other two travel until it fills.
The incoming scene sits still behind the growing window — it is *uncovered*,
not pushed, which is what separates it from `slide` and from the full-width
`wipe_*` band.

```json
{ "type": "corner_reveal", "duration": 0.5, "corner": "top_right",
  "easing": "ease_in_out" }
```

`corner` takes `top_right` (default), `top_left`, `bottom_right`, `bottom_left`
and is ignored by every other type.

`pixel_dissolve` turns the frame over cell by cell on a square lattice, each
cell **fading** on its own schedule. Mid-transition the frame is a mosaic of
both scenes with a band of half-faded cells between them — which is what
separates it from `dissolve` (one global opacity, no structure) and from the
wipes (a single hard boundary).

```json
{ "type": "pixel_dissolve", "duration": 0.7, "cell": 48, "seed": 11 }
```

| Field | Default | Notes |
| --- | --- | --- |
| `cell` | `48.0` | Cell edge in px. Smaller reads as grain, larger as blocks. |
| `seed` | `11` | Which cells turn first. Same seed → same dissolve, every render. |

Default duration: `0.5` seconds.

---

### Component Types

The engine has **57** component types total (`Component` enum, `crates/rustmotion-components/src/lib.rs:194-254`). The catalog below has a dedicated write-up with a JSON example for most of them; the rest are containers (`card`/`flex`, `div`, `grid`, `positioned`) covered in the "Mental Model: Think HTML/CSS" section above, plus `waveform`/`audio_spectrum` covered in [rules/audio-reactive.md](rules/audio-reactive.md).

All components are discriminated by `"type"`. Rendered in array order (first = bottom). See Rule 7.

#### Common Optional Fields (root level)

| Field      | Type | Default | Description                                    |
| ---------- | ---- | ------- | ---------------------------------------------- |
| `start_at` | f64  | `null`  | Show component starting at this time (seconds) |
| `end_at`   | f64  | `null`  | Hide component after this time (seconds)       |

#### Common Style Fields (inside `"style"`)

| Style field | Type | Default | Description |
| --- | --- | --- | --- |
| `width` | number or string | `null` | Component width in px, or CSS string (`"50%"`, `"auto"`) |
| `height` | number or string | `null` | Component height in px, or CSS string (`"50%"`, `"auto"`) |
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
| *preset name* | `delay`, `duration`, `loop`, `overshoot` | Any of the 40 presets (e.g. `fade_in_up`, `scale_in`) |
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

**Root fields:** `content` (required), `max_width`, `stroke`, `text-shadow`, `text-background`

| Root field         | Type     | Default    |
| ------------------ | -------- | ---------- |
| `stroke`           | object   | `null` — `{ "color": "#000", "width": 2 }` (snake_case has no inner fields to worry about) |
| `text-shadow`       | object   | `null` — single shadow, snake_case keys: `{ "color": "#000", "offset_x": 2, "offset_y": 2, "blur": 4 }` |
| `text-background`  | object   | `null` — `{ "color": "#000", "padding": 4, "corner_radius": 4 }`. See [rules/text-background.md](rules/text-background.md). |

`stroke`, `text-shadow`, and `text-background` are fields on the `text` component itself (siblings of `style`) — `CssStyle` doesn't have `stroke` or `text-background` at all, so nesting them inside `style` drops the whole component (`deny_unknown_fields`). Confusingly, `CssStyle` *does* separately define its own `text-shadow` — but as an **array** with **kebab-case** inner keys (`[{ "color": "#000", "offset-x": 2, "offset-y": 2, "blur": 4 }]`), for multi-layer shadows. Prefer the root single-shadow form shown above unless you need more than one shadow layer.

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
| `white-space`     | enum     | unset (wraps) — set `"nowrap"`/`"pre"` for single-line text. There is no `wrap` field. The validator emits `unwrappable_text_overflow` if the natural width exceeds the box. See [rules/geometry-safety.md](rules/geometry-safety.md). |
| `overflow`        | enum     | `"visible"` — CSS-like: `"visible"` (default, children may bleed) or `"hidden"` (clip at the box). Validator only checks the **viewport**, never a `visible` parent. |

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
  "fill": "#FF5733",
  "stroke": { "color": "#FFFFFF", "width": 2 },
  "style": {
    "width": 200,
    "height": 100,
    "border-radius": 16
  }
}
```

`fill` and `stroke` are **root fields**, not CSS — placing them inside `style` fails with `unknown field` (`CssStyle` is `deny_unknown_fields`) and the shape is silently dropped. See [rules/component-field-placement.md](rules/component-field-placement.md).

**Root fields:** `shape` (required), `text`, `fill`, `stroke`

| Root field | Type               | Default     |
| --------------- | ------------------ | ----------- |
| `fill`          | string or gradient | `null`      |
| `stroke`        | `{color, width}`   | `null`      |

| Style field     | Type               | Default     |
| --------------- | ------------------ | ----------- |
| `border-radius` | f32                | `null`      |

**Shape types.** `ShapeType` is externally tagged: the plain variants are strings, the parameterised ones are single-key objects. Writing `"shape": "star"` fails with `invalid type: unit variant, expected struct variant`.

```json
"shape": "rect"                              // also: circle, rounded_rect, ellipse, triangle
"shape": { "star": { "points": 6 } }         // default 5
"shape": { "polygon": { "sides": 6 } }       // default 6
"shape": { "path": { "data": "M0 0 L10 10" } }
```

**Gradient fill (root field):**
```json
{
  "type": "shape",
  "shape": "circle",
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
  "fit": "cover",
  "style": { "width": 400, "height": 300 }
}
```

| Field      | Type   | Default                                                         |
| ---------- | ------ | --------------------------------------------------------------- |
| `src`      | string | required — path to image file                                   |
| `position` | `{x, y}` | `{0, 0}`                                                      |
| `fit`      | enum   | `"cover"` — options: `"cover"`, `"contain"`, `"fill"`, `"none"` |

Style: `width`, `height` (default: uses image dimensions)

### 4. `svg`

```json
{
  "type": "svg",
  "data": "<svg>...</svg>",
  "style": { "width": 200, "height": 200 }
}
```

| Field      | Type     | Default                                                     |
| ---------- | -------- | ----------------------------------------------------------- |
| `src`      | string   | `null` — path to SVG file (either `src` or `data` required) |
| `data`     | string   | `null` — inline SVG markup                                  |
| `position` | `{x, y}` | `{0, 0}`                                                    |

Style: `width`, `height` (default: intrinsic SVG dimensions)

### 5. `icon`

Renders an icon from the **Iconify** open-source framework (200,000+ icons from 150+ sets). Icons are fetched from the Iconify API at render time. Browse all icons: https://icon-sets.iconify.design/

```json
{
  "type": "icon",
  "icon": "lucide:home",
  "style": { "width": 64, "height": 64, "color": "#38bdf8" }
}
```

| Field      | Type     | Default                                                      |
| ---------- | -------- | ------------------------------------------------------------ |
| `icon`     | string   | required — Iconify id `"prefix:name"` (e.g. `"lucide:home"`) |
| `position` | `{x, y}` | `{0, 0}`                                                     |

Style: `width`, `height` (default `24`), `color` (default `"#FFFFFF"`)

Common prefixes: `lucide` (UI), `mdi` (Material), `heroicons`, `ph` (Phosphor), `tabler`, `simple-icons` (brand logos), `devicon` (dev tools)

### 6. `video`

```json
{
  "type": "video",
  "src": "path/to/video.mp4",
  "trim_start": 2.0,
  "trim_end": 10.0,
  "style": { "width": 1920, "height": 1080 }
}
```

| Field           | Type     | Default   |
| --------------- | -------- | --------- |
| `src`           | string   | required  |
| `position`      | `{x, y}` | `{0, 0}`  |
| `trim_start`    | f64      | `null`    |
| `trim_end`      | f64      | `null`    |
| `playback_rate` | f64      | `null`    |
| `fit`           | enum     | `"cover"` |
| `volume`        | f32      | `1.0`     |
| `loop_video`    | bool     | `null`    |

Style: `width`, `height` (required)

### 7. `gif`

```json
{
  "type": "gif",
  "src": "path/to/animation.gif",
  "style": { "width": 200, "height": 200 }
}
```

| Field      | Type     | Default   |
| ---------- | -------- | --------- |
| `src`      | string   | required  |
| `position` | `{x, y}` | `{0, 0}`  |
| `fit`      | enum     | `"cover"` |
| `loop_gif` | bool     | `true`    |

Style: `width`, `height` (default: intrinsic GIF dimensions)

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

Animated number counter. Works fine inside a card (centers correctly) — see checklist item 4 / [rules/counter-standalone.md](rules/counter-standalone.md) for sizing the parent to its worst-case digit width.

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
  "style": { "font-size": 72, "color": "#FFFFFF", "font-weight": "bold", "text-align": "center" }
}
```

`end_at` is a visibility toggle, not an animation-completion boundary — setting it on a `counter` makes the number **disappear** once that time passes, since the counter's own animation is driven by `ctx.time / scene_duration`, not by `start_at`/`end_at`. Use `start_at` only.

**Root fields:** `from`, `to`, `decimals`, `separator`, `prefix`, `suffix`, `easing`

**Easing options:** `linear`, `ease_in`, `ease_out`, `ease_in_out`, `ease_in_quad`, `ease_out_quad`, `ease_in_cubic`, `ease_out_cubic`, `ease_in_expo`, `ease_out_expo`, `spring`

Style: `font-size` (48.0), `color` (#FFFFFF), `font-family` (Inter), `font-weight`, `text-align`, `letter-spacing`, `text-shadow`, `stroke`

### 10. Absolute Positioning (via `card`)

To place children at fixed absolute coordinates, use a `card` with transparent background and explicit size. Each child uses `position: {x, y}` relative to the card's top-left. Children with `position` become absolute inside a `card`.

```json
{
  "type": "card",
  "style": { "width": 1920, "height": 1080, "background": "#00000000", "padding": 0 },
  "children": [
    { "type": "shape", "shape": "rect", "fill": "#1E293B", "position": { "x": 0, "y": 0 }, "style": { "width": 400, "height": 300, "border-radius": 16 } },
    { "type": "icon", "icon": "lucide:phone-off", "position": { "x": 170, "y": 120 }, "style": { "width": 64, "height": 64, "color": "#FFFFFF" } }
  ]
}
```

### 11. `card` / `flex`

Visual container with CSS-like flex & grid layout. `flex` is an alias for `card`. See Rule 8.

Each dimension (`width`/`height` in `style`) can be a number or `"auto"`.

**Flex example:**
```json
{
  "type": "card",
  "style": { "width": 800, "height": 100, "flex-direction": "row", "gap": 16 },
  "children": [
    { "type": "shape", "shape": "rect", "fill": "#FF0000", "style": { "width": 100, "height": 100 } },
    { "type": "shape", "shape": "rect", "fill": "#00FF00", "style": { "width": 100, "height": 100, "flex-grow": 1 } },
    { "type": "shape", "shape": "rect", "fill": "#0000FF", "style": { "width": 100, "height": 100 } }
  ]
}
```

**Grid example (2x2):** Note: grid containers need explicit `height` (not `"auto"`) — see [rules/grid-card-height.md](rules/grid-card-height.md). `grid-template-columns`/`grid-template-rows` is `Vec<GridTrack>`, an **untagged** enum: a bare number means px, a quoted string like `"1fr"` carries the unit, `"auto"` is the keyword. The object forms `{"fr": N}` / `{"px": N}` shown in older docs do **not** match any variant and drop the whole component.
```json
{
  "type": "card",
  "style": {
    "width": 600,
    "height": 400,
    "display": "grid",
    "grid-template-columns": ["1fr", "1fr"],
    "grid-template-rows": ["1fr", "1fr"],
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
| `box-shadow`             | array       | `null` — `[{ "color": "#00000040", "offset-x": 0, "offset-y": 4, "blur": 12 }]` (kebab-case keys, always an array — see [rules/component-field-placement.md](rules/component-field-placement.md)) |
| `padding`                | f32 or obj  | `null`     |
| `flex-direction`         | enum        | `"column"` — `"row"`, `"row-reverse"`, `"column"`, `"column-reverse"` (kebab-case) |
| `flex-wrap`              | enum        | `"nowrap"` — `"nowrap"`, `"wrap"`, `"wrap-reverse"` (NOT a bool) |
| `align-items`            | enum        | `"start"` — `"start"`, `"center"`, `"end"`, `"stretch"`, `"flex-start"`, `"flex-end"`, `"baseline"` |
| `justify-content`        | enum        | `"start"` — `"start"`, `"center"`, `"end"`, `"space-between"`, `"space-around"`, `"space-evenly"` (kebab-case) |
| `gap`                    | f32         | `0`        |
| `grid-template-columns`  | array       | `null` — `["1fr", 200, "auto"]` (bare number = px, quoted `"Nfr"` = fr, `"auto"` = keyword) |
| `grid-template-rows`     | array       | `null`     |

**Per-child layout properties** (in child `"style"`):
- `flex-grow` (f32) — default 0
- `flex-shrink` (f32) — default 1
- `flex-basis` (f32) — defaults to natural size
- `align-self` (enum) — `"start"`, `"center"`, `"end"`, `"stretch"`
- `grid-column` (object) — `{ "start": 1, "span": 2 }` (1-indexed)
- `grid-row` (object) — `{ "start": 1, "span": 2 }` (1-indexed)

`position: "absolute"` (root field, sibling of `style`) works on a child of **any** container — `card`, `div`, `grid`, `positioned`, or the scene root — not just inside `positioned`. `positioned` is simply a semantic Stack-like container with no visual decoration; it does not unlock `position` — every container already supports it. Children without `position` are laid out using flex/grid style properties.

### 12. `div`

Invisible flex wrapper — groupe des enfants pour un layout pur ou des transforms partagés. Comme `card`/`flex` mais **sans background, border, shadow, ni clipping**. Équivalent de `<div>` en HTML.

Utiliser `div` quand il faut grouper des éléments sans décoration visuelle (ex: grille de cards, ligne d'icônes, animation partagée sur un groupe).

```json
{
  "type": "div",
  "style": {
    "flex-direction": "column",
    "align-items": "center",
    "gap": 36
  },
  "timeline": [
    { "at": 3.5, "animation": [{ "name": "keyframes", "keyframes": [
      { "property": "scale", "keyframes": [{ "time": 0, "value": 1 }, { "time": 0.8, "value": 4 }], "easing": "ease_in" },
      { "property": "opacity", "keyframes": [{ "time": 0, "value": 1 }, { "time": 0.7, "value": 0 }], "easing": "ease_in" }
    ]}]}
  ],
  "children": [
    { "type": "icon", "icon": "lucide:zap", "style": { "width": 80, "height": 80, "color": "#25D366" } },
    { "type": "text", "content": "Grouped content", "style": { "font-size": 48, "color": "#FFFFFF" } }
  ]
}
```

`timeline` and `stagger` are **root fields**, not `style` — `CssStyle` has no `timeline` key and `deny_unknown_fields` drops the whole component if you nest it there. Supporte toutes les propriétés CSS flex/grid (`flex-direction`, `align-items`, `justify-content`, `gap`, `padding`, `display: "grid"`, `grid-template-columns`) dans `style`, plus `timeline`/`stagger` au niveau racine. Préférer `div` à `card` avec fond transparent pour tout layout sans styling visuel.

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

**Root fields:** `code` (required), `language`, `theme`, `show_line_numbers`, `chrome`, `highlights`, `reveal`, `states`, `diff` (bool — enables diff mode: lines starting with `+` get green background, `-` get red background), `auto_scroll` (bool, default `true` — when content overflows the box vertically, scrolls so the last revealed line stays visible; font is never reduced. See [rules/geometry-safety.md](rules/geometry-safety.md))

Style: `width`, `height` (set to constrain the visible area; content scrolls if it overflows vertically when `auto_scroll: true`)

**Diff mode example:**
```json
{
  "type": "codeblock",
  "language": "diff",
  "diff": true,
  "code": " fn render() {\n-    let old = bar();\n+    let new = donut();\n }",
  "chrome": { "enabled": true, "title": "changes.rs" }
}
```

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

Compact pill-shaped label with optional icon, dot indicator, pulse animation, and count badge.

```json
{
  "type": "badge",
  "text": "Messages",
  "icon": "lucide:mail",
  "variant": "solid",
  "badge_size": "lg",
  "dot": true,
  "dot_color": "#22C55E",
  "pulse": true,
  "count": 12,
  "style": { "background": "#3B82F6" }
}
```

**Root fields:** `text` (required), `icon` (Iconify id), `variant` (solid/outline), `badge_size` (sm/md/lg), `dot` (bool — colored dot top-right), `dot_color` (hex, defaults to badge color), `pulse` (bool — animated pulse ring on dot), `count` (u32 — red count badge top-right, caps at "99+")

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
  "style": { "width": 600, "height": 300 }
}
```

**Root fields:** `lines` (required — `[{ "text", "line_type", "color" }]`), `theme` (dark/light), `title`, `show_chrome` (default true), `reveal`, `auto_scroll` (bool, default `true` — vertical scroll when content > box, font never shrinks. See [rules/geometry-safety.md](rules/geometry-safety.md))

Style: `width`, `height` (set to constrain the visible area)

**Reveal:** `{ "mode": "typewriter"|"line_by_line", "start": 0, "duration": 1.0, "easing": "linear" }` — animates line/word appearance. In typewriter mode, a blinking cursor appears at the typing position.

Line types: `"prompt"` ($ prefix in green), `"command"` (white), `"output"` (gray)

Style: `font-size` (default 14)

### 18. `table`

Data table with headers, styled rows, configurable column widths and alignment.

```json
{
  "type": "table",
  "headers": ["Metric", "Value", "Change"],
  "rows": [["Revenue", "1.2M", "+24%"], ["Users", "45K", "+12%"]],
  "column_widths": [300, 200, 150],
  "column_align": ["left", "right", "right"],
  "cell_padding": 20,
  "show_borders": true,
  "style": { "width": 650, "height": 150, "color": "#E2E8F0", "font-size": 15, "border-radius": 12 }
}
```

**Root fields:** `headers` (required), `rows` (required), `header_color` (#374151), `row_colors` (alternating array), `border_color` (#4B5563), `header_text_color`, `column_widths` (array of f32 — explicit pixel widths per column), `column_align` (array — `"left"` / `"center"` / `"right"` per column), `cell_padding` (f32, default 12), `show_borders` (bool, default true)

Style: `color` (default `"#FFFFFF"`) — cell text color, `font-size` (default 14), `font-family`, `border-radius`

### 19. `chart`

Data visualization with animation. Supports 12 chart types.

```json
{
  "type": "chart",
  "chart_type": "area",
  "data": [
    { "value": 10, "label": "Jan" },
    { "value": 25, "label": "Feb" },
    { "value": 18, "label": "Mar" },
    { "value": 42, "label": "Apr" }
  ],
  "smooth": true,
  "fill_opacity": 0.3,
  "show_grid": true,
  "show_x_labels": true,
  "show_y_labels": true,
  "style": { "width": 600, "height": 300 }
}
```

**Chart types:** `bar`, `line`, `pie`, `donut`, `horizontal_bar`, `area`, `stacked_bar`, `radar`, `scatter`, `radial_bar`, `funnel`, `waterfall`

**Root fields:** `chart_type` (required), `data` (`[{ "value", "label"?, "color"? }]`), `animated` (default true), `animation_duration` (default 1.5s), `colors` (custom palette)

Style: `width`, `height` — **required**. `chart` has no intrinsic sizing (no fallback in the engine — verified against `box_builder.rs`'s `component_intrinsic`/`apply_intrinsic_overrides`); omit them in a flex/grid container and the chart lays out at 0×0 and renders nothing.

**Axes & grid (bar, line, area, stacked_bar, scatter, waterfall):** `show_grid`, `show_x_labels`, `show_y_labels`, `grid_color` (#FFFFFF15), `label_color` (#888888), `label_font_size` (12)

**Type-specific fields:**

| Field | Chart Types | Default | Description |
| --- | --- | --- | --- |
| `inner_radius` | donut | `0.6` | Hole size ratio (0.1–0.95) |
| `fill_opacity` | area | `0.3` | Gradient fill opacity |
| `smooth` | area | `false` | Catmull-Rom spline smoothing |
| `show_labels` | horizontal_bar, funnel | `false` | Labels inside bars/segments |
| `direction` | funnel | `"vertical"` | `"vertical"` or `"horizontal"` |
| `categories` | stacked_bar | `[]` | X-axis category names |
| `series` | stacked_bar | `[]` | `[{ "name", "data": [f64], "color"? }]` |
| `axes` | radar | `[]` | Axis labels |
| `radar_data` | radar | `[]` | `[{ "values": [f64], "color"? }]` |
| `points` | scatter | `[]` | `[{ "x", "y", "size"?, "color"? }]` |

**Stacked bar example:**
```json
{
  "type": "chart",
  "chart_type": "stacked_bar",
  "categories": ["Q1", "Q2", "Q3", "Q4"],
  "series": [
    { "name": "Product A", "data": [30, 40, 35, 50], "color": "#3B82F6" },
    { "name": "Product B", "data": [20, 15, 25, 30], "color": "#22C55E" }
  ],
  "show_grid": true, "show_x_labels": true, "show_y_labels": true
}
```

**Funnel example (horizontal):**
```json
{
  "type": "chart",
  "chart_type": "funnel",
  "direction": "horizontal",
  "data": [
    { "value": 10000, "label": "Visitors", "color": "#3B82F6" },
    { "value": 6500, "label": "Leads", "color": "#6366F1" },
    { "value": 3200, "label": "Qualified", "color": "#8B5CF6" }
  ],
  "show_labels": true
}
```

**Waterfall** uses green for positive values, red for negative, with dashed connectors between bars.

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
  "speed": 1.0,
  "loop": true,
  "style": { "width": 300, "height": 300 }
}
```

| Field        | Type   | Default | Description                                                  |
| ------------ | ------ | ------- | ------------------------------------------------------------ |
| `src`        | string | `null`  | Path to Lottie JSON file (for metadata: fps, frame count)    |
| `data`       | string | `null`  | Inline Lottie JSON data (alternative to `src`)               |
| `frames_dir` | string | `null`  | Directory with pre-rendered frames (`0000.png`, `0001.png`, ...) |
| `speed`      | f32    | `1.0`   | Playback speed multiplier                                    |
| `loop`       | bool   | `true`  | Loop the animation                                           |

Style: `width`, `height` (default: Lottie intrinsic size)

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
| `cursor_style`  | string | `"default"`   | `"default"` or `"pointer"` — metadata only: both draw the same bar |
| `path_easing`   | string | `"ease_in_out"` | Path interpolation: `"linear"`, `"ease_out"`, `"ease_in_out"`, `"step"` |

> The component draws a **caret** (a rounded vertical bar), not an arrow. Staged as a
> text caret it should use `"path_easing": "step"`, which holds each waypoint and jumps
> to the next — a caret never slides between two fields. The interpolating easings are
> for a pointer travelling over a surface.

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

### 29. `progress`

Progress bar with linear (default) or circular variant.

```json
{
  "type": "progress",
  "progress": 0.75,
  "variant": "circular",
  "width": 120,
  "height": 120,
  "fill_color": "#3B82F6",
  "background_color": "#1E293B",
  "track_width": 8,
  "show_value": true
}
```

**Root fields:** `progress` (0.0–1.0), `variant` (`"linear"` or `"circular"`), `width` (default 300), `height` (default 20 linear / same as width circular), `fill_color` (#4CAF50), `background_color` (#333333), `border_radius` (linear only), `track_width` (circular only, default 8), `show_value` (circular only — shows percentage text)

### 30. `gauge`

Semi-circular arc gauge for KPIs and dashboards.

```json
{
  "type": "gauge",
  "value": 72,
  "max": 100,
  "label": "Performance",
  "fill_color": "#3B82F6",
  "track_color": "#1E293B",
  "track_width": 16,
  "show_value": true,
  "style": { "width": 200, "height": 140 }
}
```

**Root fields:** `value` (required), `min` (0), `max` (100), `label`, `fill_color` (#3B82F6), `track_color` (#333333), `track_width` (16), `start_angle` (135), `end_angle` (405), `show_value` (true), `animated` (true), `animation_duration` (1.5s)

Style: `width`, `height` — **required**, same as `chart`: `gauge` has no intrinsic sizing; without explicit dimensions it lays out at 0×0.

### 31. `sparkline`

Mini inline chart without axes — ideal inside cards next to counters.

```json
{
  "type": "sparkline",
  "data": [5, 12, 8, 20, 15, 25, 18, 30],
  "color": "#22C55E",
  "fill": true,
  "fill_opacity": 0.2,
  "stroke_width": 2,
  "style": { "width": 120, "height": 40 }
}
```

**Root fields:** `data` (required — array of f64), `color` (#22C55E), `fill` (false — gradient fill under line), `fill_opacity` (0.2), `stroke_width` (2.0), `animated` (true), `animation_duration` (1.0s)

Style: `width`, `height` — **required**: `sparkline` has no intrinsic sizing (confirmed empirically — omitted, it renders zero pixels); always set explicit dimensions, e.g. `120×40`.

### 32. `stat`

Composite KPI card: value + label + trend arrow + sparkline.

```json
{
  "type": "stat",
  "value": "45.2K",
  "label": "Active Users",
  "trend": { "value": "+12.5%", "direction": "up" },
  "sparkline_data": [20, 25, 22, 30, 28, 35, 32, 40, 38, 45],
  "sparkline_color": "#22C55E",
  "style": { "width": 280, "height": 180, "background": "#1E293B", "border-radius": 16 }
}
```

**Root fields:** `value` (required — display string), `label`, `trend` (`{ "value": string, "direction": "up"/"down"/"neutral", "color"? }`), `sparkline_data` (array of f64), `sparkline_color`, `value_font_size` (48), `label_font_size` (14), `value_color` (#FFFFFF), `label_color` (#94A3B8)

Style: `width`, `height` — **required**: `stat` has no intrinsic sizing. Verified empirically — three `stat`s in a flex-row card with no explicit `width`/`height` render **zero pixels** (all three collapse to 0×0). Always set explicit dimensions, e.g. `280×180`. See [rules/stat-cards.md](rules/stat-cards.md).

Trend uses `lucide:trending-up` / `lucide:trending-down` icons. Direction `"down"` with a positive connotation (e.g. churn decreasing) can use `"color": "#22C55E"` to override the default red.

### 33. `skeleton`

Loading placeholder with animated shimmer effect. Three variants for different content types.

```json
{
  "type": "skeleton",
  "variant": "text",
  "lines": 3,
  "style": { "width": 300, "height": 68 }
}
```

**Root fields:** `variant` (`"rectangle"` / `"circle"` / `"text"`), `base_color` (#1E293B), `shimmer_color` (#334155), `border_radius` (8), `speed` (1.5 — shimmer cycle duration), `lines` (3 — text variant only), `line_height` (16), `line_gap` (12)

Style: `width`, `height` (default: rectangle 200×40, circle 48×48, text auto-computed from lines)

Default sizes: rectangle 200x40, circle 48x48, text auto-computed from lines.

### 34. `kbd`

Visual keyboard key — for documenting shortcuts.

```json
{
  "type": "kbd",
  "key": "Cmd"
}
```

**Root fields:** `key` (required — text displayed), `font_size` (14), `background_color` (#1E293B), `border_color` (#475569), `text_color` (#E2E8F0)

Auto-sizes based on text content. Has a 3D depth effect (shadow below). Uses monospace font. Style overrides: `background`, `color`, `font-size`.

### 35. `tooltip`

Floating label with directional arrow — for annotations and callouts.

```json
{
  "type": "tooltip",
  "text": "Click to expand",
  "arrow": "bottom",
  "background_color": "#1E293B",
  "text_color": "#E2E8F0",
  "border_color": "#334155"
}
```

**Root fields:** `text` (required), `arrow` (`"top"` / `"bottom"` / `"left"` / `"right"` / `"none"`, default `"bottom"`), `font_size` (13), `background_color` (#1E293B), `text_color` (#E2E8F0), `arrow_size` (8), `border_color` (optional)

Style overrides: `background`, `color`, `font-size`, `border-radius` (8).

### 36. `marquee`

Continuous scrolling text — for tickers, breaking news, or decorative text bands.

```json
{
  "type": "marquee",
  "content": "Breaking news — rustmotion 2.0 released!",
  "speed": 100,
  "direction": "left",
  "font_size": 24,
  "color": "#3B82F6",
  "style": { "width": 800, "height": 48 }
}
```

**Root fields:** `content` (required), `speed` (100 — pixels/second), `direction` (`"left"` / `"right"`), `font_size` (24), `color` (#FFFFFF), `separator` (spacing between repeats, default 5 spaces)

Style: `width`, `height` — **required**: `marquee` has no intrinsic sizing; always set explicit dimensions (it's exempt from viewport-overflow checks since its role is to bleed, but it still needs a box to scroll within).

### 37. `avatar_group`

Stacked circular avatars with overlap and "+N" overflow badge.

```json
{
  "type": "avatar_group",
  "avatars": [
    { "src": "user1.png" },
    { "src": "user2.png" },
    { "src": "user3.png" },
    { "src": "user4.png" },
    { "src": "user5.png" }
  ],
  "max_display": 3,
  "overlap": 16,
  "size": 48
}
```

**Root fields:** `avatars` (required — `[{ "src": string }]`), `max_display` (optional — limit visible avatars), `size` (48 — diameter), `overlap` (16 — px overlap between avatars), `border_width` (3), `border_color` (#0f172a — ring color, match your background)

### 38. `switch`

Animated toggle switch that flips state at a configurable time.

```json
{
  "type": "switch",
  "value": false,
  "toggle_at": 1.5,
  "label": "Dark Mode",
  "width": 52,
  "height": 28,
  "track_color_on": "#4CAF50",
  "track_color_off": "#CCCCCC"
}
```

**Root fields:** `value` (bool, default false), `toggle_at` (time to flip), `label`, `width` (52), `height` (28), `track_color_on` (#4CAF50), `track_color_off` (#CCCCCC), `thumb_color` (#FFFFFF), `transition_duration` (0.3)

### 39. `slider`

Horizontal slider that animates to a target value.

```json
{
  "type": "slider",
  "value": 0.3,
  "animate_to": 0.85,
  "animate_at": 1.0,
  "animation_duration": 2.0,
  "width": 300,
  "fill_color": "#3B82F6",
  "show_value": true
}
```

**Root fields:** `value` (0.0–1.0), `animate_to`, `animate_at` (time to start), `animation_duration` (1.0), `width` (300), `height` (8), `track_color` (#333333), `fill_color` (#3B82F6), `thumb_size` (20), `thumb_color` (#FFFFFF), `show_value` (false)

### 40. `rating`

Star rating display with animated fill.

```json
{
  "type": "rating",
  "value": 4.5,
  "max": 5,
  "size": 32,
  "filled_color": "#F59E0B"
}
```

**Root fields:** `value` (f64), `max` (5), `size` (32 — star diameter), `gap` (4), `filled_color` (#F59E0B), `empty_color` (#374151), `animated` (true), `animation_duration` (1.0)

### 41. `gradient_text`

Text with animated gradient fill.

```json
{
  "type": "gradient_text",
  "content": "Build Faster",
  "colors": ["#3B82F6", "#8B5CF6", "#EC4899"],
  "angle": 90,
  "animate_angle": true,
  "speed": 0.3,
  "style": { "font-size": 72, "font-weight": "bold" }
}
```

**Root fields:** `content` (required), `colors` (array of hex, default ["#3B82F6", "#8B5CF6"]), `angle` (90 — gradient angle in degrees), `animate_angle` (false — rotate gradient over time), `speed` (0.5 — rotations/sec when animate_angle), `size`

Style: `font-size`, `font-weight`, `font-family`

### 42. `list`

Feature list with bullet, numbered, or checklist items.

```json
{
  "type": "list",
  "items": [
    { "text": "Unlimited projects", "icon": "lucide:check" },
    { "text": "Priority support", "icon": "lucide:check" },
    { "text": "Advanced analytics", "icon": "lucide:x" }
  ],
  "variant": "checklist",
  "icon_color": "#22C55E",
  "unchecked_color": "#EF4444",
  "gap": 16,
  "width": 400,
  "style": { "font-size": 18, "color": "#E2E8F0" }
}
```

**Root fields:** `items` (required — `[{ "text", "icon"?, "checked"? }]`), `variant` (`"bullet"` / `"numbered"` / `"checklist"`), `gap` (16), `icon_size` (20), `icon_color` (#22C55E), `unchecked_color` (#6B7280), `width` (400)

### 43. `pill_nav`

Horizontal tab navigation with animated pill indicator.

```json
{
  "type": "pill_nav",
  "items": ["Overview", "Analytics", "Settings"],
  "active_index": 0,
  "transitions": [
    { "to": 1, "at": 2.0 },
    { "to": 2, "at": 4.0 }
  ],
  "pill_color": "#3B82F6",
  "height": 44
}
```

**Root fields:** `items` (required — array of strings), `active_index` (0), `transitions` (`[{ "to": u32, "at": f64 }]`), `pill_color` (#3B82F6), `text_color` (#FFFFFF), `inactive_text_color` (#9CA3AF), `background_color` (#1E293B), `height` (44), `border_radius` (22), `gap` (4), `transition_duration` (0.3)

### 44. `notification`

Toast notification with fade-in/out and stack push animation.

```json
{
  "type": "notification",
  "title": "Deployment Complete",
  "message": "v2.4.1 deployed to production",
  "variant": "success",
  "width": 380,
  "slide_in_at": 0.8,
  "slide_out_at": 4.5,
  "push_at": [1.5],
  "position": "absolute",
  "x": 500,
  "y": 100
}
```

**Root fields:** `title` (required), `message`, `icon` (Iconify id), `variant` (info/success/warning/error), `width` (360), `slide_in_at` (0.5 — fade-in time), `slide_out_at` (fade-out time), `slide_duration` (0.15 — fade speed), `accent_color` (override variant color), `push_at` (array of timestamps — when to push down one slot), `stack_gap` (12), `wait_for_push` (bool — delay fade-in until push animation finishes)

**Stacking notifications:** Place all at the same x/y. The first notification gets `push_at: [1.5]` (time when second appears). The second gets `wait_for_push: true`. This makes the first slide down, then the second fades in above it.

### 45. `stepper`

Step indicator with connected nodes and animated progression.

```json
{
  "type": "stepper",
  "steps": [
    { "label": "Sign Up" },
    { "label": "Configure" },
    { "label": "Deploy" }
  ],
  "active_step": 0,
  "animate_to": 2,
  "animate_at": 1.0,
  "style": { "width": 600, "height": 80 }
}
```

**Root fields:** `steps` (required — `[{ "label", "description"? }]`), `active_step` (0), `animate_to` (target step), `animate_at` (time), `transition_duration` (0.5), `orientation` ("horizontal"), `active_color` (#3B82F6), `completed_color` (#22C55E), `pending_color` (#6B7280), `node_size` (32)

Style: `width`, `height`

### 46. `comparison`

Before/after split view with animated divider.

```json
{
  "type": "comparison",
  "left_color": "#1E293B",
  "right_color": "#3B82F6",
  "left_label": "Before",
  "right_label": "After",
  "divider_position": 0.5,
  "animate_from": 0.2,
  "animate_to": 0.8,
  "animate_at": 1.0,
  "animation_duration": 2.0,
  "border_radius": 16,
  "style": { "width": 600, "height": 300 }
}
```

**Root fields:** `left_color`, `right_color`, `left_label`, `right_label`, `divider_position` (0.5), `animate_from`, `animate_to`, `animate_at`, `animation_duration` (2.0), `divider_color` (#FFFFFF), `divider_width` (3), `border_radius` (12)

Style: `width`, `height`

### 47. `countdown`

Digital countdown timer with flip-clock style digit boxes.

```json
{
  "type": "countdown",
  "seconds": 3723,
  "digit_size": 48,
  "digit_color": "#FFFFFF",
  "digit_background": "#1E293B",
  "style": { "width": 400, "height": 80 }
}
```

**Root fields:** `seconds` (total countdown, counts down from ctx.time), `show_hours` (true), `show_minutes` (true), `show_seconds` (true), `digit_size` (64), `digit_color` (#FFFFFF), `digit_background` (#1E293B), `separator_color` (#6B7280), `gap` (12), `border_radius` (12)

Style: `width`, `height`

### 48. `heatmap`

Grid of colored cells (GitHub contribution style).

```json
{
  "type": "heatmap",
  "data": [
    [0.1, 0.5, 0.9, 0.3, 0.7],
    [0.4, 0.8, 0.2, 0.6, 0.5]
  ],
  "cell_size": 20,
  "cell_gap": 3,
  "cell_radius": 4,
  "style": { "width": 400, "height": 200 }
}
```

**Root fields:** `data` (required — 2D array of f64, values 0.0–1.0), `color_scale` (array of hex, default GitHub green scale), `cell_size` (14), `cell_gap` (3), `cell_radius` (2), `animated` (true), `animation_duration` (1.5)

Style: `width`, `height`

### 49. `treemap`

Space-filling rectangles proportional to values.

```json
{
  "type": "treemap",
  "data": [
    { "label": "React", "value": 45, "color": "#61DAFB" },
    { "label": "Vue", "value": 25, "color": "#42B883" },
    { "label": "Angular", "value": 20, "color": "#DD0031" }
  ],
  "show_labels": true,
  "gap": 3,
  "border_radius": 6,
  "style": { "width": 500, "height": 300 }
}
```

**Root fields:** `data` (required — `[{ "label"?, "value", "color"? }]`), `gap` (3), `border_radius` (6), `show_labels` (true), `show_values` (false), `animated` (true), `animation_duration` (1.0)

Style: `width`, `height`

### 50. `tag_cloud`

Word cloud with weighted font sizes.

```json
{
  "type": "tag_cloud",
  "tags": [
    { "text": "Rust", "weight": 10 },
    { "text": "TypeScript", "weight": 8 },
    { "text": "Python", "weight": 7 }
  ],
  "min_font_size": 14,
  "max_font_size": 64,
  "style": { "width": 500, "height": 300 }
}
```

**Root fields:** `tags` (required — `[{ "text", "weight", "color"? }]`), `min_font_size` (14), `max_font_size` (64), `colors` (custom palette), `animated` (true), `animation_duration` (1.5)

Style: `width`, `height`

### 51. `dot_map`

World map in dot-pattern with data points at geographic coordinates.

```json
{
  "type": "dot_map",
  "points": [
    { "lat": 40.71, "lng": -74.01, "label": "NYC", "size": 12, "color": "#3B82F6", "pulse": true },
    { "lat": 35.68, "lng": 139.69, "label": "Tokyo", "size": 14, "color": "#EF4444", "pulse": true },
    { "lat": -33.87, "lng": 151.21, "label": "Sydney", "size": 10, "color": "#EC4899" }
  ],
  "show_world": true,
  "world_dot_color": "#334155",
  "dot_spacing": 10,
  "dot_radius": 2,
  "style": { "width": 800, "height": 500 }
}
```

**Root fields:** `points` (required — `[{ "lat", "lng", "label"?, "size"?, "color"?, "pulse"? }]`), `show_world` (true — show world map background), `world_dot_color` (#334155), `dot_spacing` (8 — grid spacing px), `dot_radius` (1.5), `background_color` (#0F172A), `animated` (true), `animation_duration` (1.5)

Style: `width`, `height`

Points use real geographic coordinates (lat/lng). The world map is rendered as a dot grid using a 180×90 land bitmap. Points with `pulse: true` show expanding concentric rings.

### 52. `qr_code`

Renders a scannable QR code from arbitrary content (URL, text, etc.).

```json
{
  "type": "qr_code",
  "content": "https://rustmotion.dev",
  "size": 240,
  "foreground_color": "#0F172A",
  "background_color": "#FFFFFF"
}
```

**Root fields:** `content` (required), `size` (default 200 — used as both width and height unless overridden), `foreground_color` (#000000), `background_color` (#FFFFFF)

Style: `width`, `height` — optional; falls back to `size` × `size` via `apply_intrinsic_overrides` if unset.

The JSON `"type"` value is **`qr_code`** (snake_case of the `QrCode` enum variant), not `qrcode`.

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

The `background` field on a scene accepts a color string, an animated background object (inline or via `$ref`), or an array of layered backgrounds. The legacy `animated-background` field is still supported.

**Prefer `$ref` templates** when the same background is reused across scenes — define once in `backgrounds`, reference everywhere.

```json
{
  "backgrounds": {
    "circles": { "preset": "concentric_circles", "colors": ["#0F0E2A", "#1a1145", "#0F0E2A"], "speed": 15, "element_size": 1.5, "count": 4, "gradient_type": "radial" }
  },
  "scenes": [
    {
      "duration": 5.0,
      "background": { "$ref": "circles" },
      "children": [...]
    },
    {
      "duration": 5.0,
      "background": { "$ref": "circles", "colors": ["#1a0a2e", "#2d1b69", "#1a0a2e"], "transition": { "duration": 1.0, "easing": "ease_in_out" } },
      "children": [...]
    }
  ]
}
```

With `transition`, background properties (colors, speed, spacing, element_size, zones) interpolate smoothly from the previous scene's values.

| Field          | Type   | Default           | Description                                   |
| -------------- | ------ | ----------------- | --------------------------------------------- |
| `colors`       | array  | `[]`              | Gradient colors (hex)                         |
| `speed`        | f32    | `30.0`            | Animation speed (degrees/sec or pixels/sec)   |
| `gradient_type`| enum   | `"linear"`        | `"linear"` or `"radial"`                      |
| `preset`       | string | `null`            | `"gradient_shift"`, `"concentric_circles"`, `"grid_dots"`, `"halo"`, `"heropattern"`, `"pixel_grid"` |
| `element_size` | f32    | `4.0`             | Dot/circle size for grid_dots; stroke width for concentric_circles |
| `spacing`      | f32    | `60.0`            | Element spacing for grid_dots/concentric_circles |
| `count`        | u32    | `null`            | Number of circles for concentric_circles (overrides spacing) |
| `zones`        | array  | `[]`              | `halo` only — `[{ "color": "#hex", "x": 0.0-1.0, "y": 0.0-1.0, "radius": 0.0-1.0 }]`. `x`/`y` are fractions of width/height, `radius` a fraction of `max(width, height)`. |
| `$ref`         | string | `null`            | Reference to a named template in `backgrounds` |
| `transition`   | object | `null`            | `{ "duration": f64, "easing": "ease_in_out" }` — interpolates from prev scene |

### `pixel_grid` — a lattice of square cells

Two looks from one preset. **Sparse tile field**: one colour under `density: 1`,
cells scattered by a hash of their coordinates. **Checkerboard**: two colours at
`density: 1.0`, which alternate by `(col + row)`.

```json
{ "preset": "pixel_grid", "speed": 1.0, "pixel_grid": {
    "colors": ["#FFFFFF26"],   // one → field; two+ → alternating checkerboard
    "size": 9,                 // cell edge, px
    "spacing": 22,             // lattice pitch, px — clamped to at least `size`
    "density": 0.75,           // 0..1 fraction of cells drawn
    "density_ramp": "edges",   // none | left | right | top | bottom | radial | edges
    "radius": 1,               // cell corner radius; 0 for hard pixels
    "seed": 7,                 // stable scatter; same seed → same pattern
    "motion": "none"           // none | twinkle | sweep
} }
```

| Field | Default | Notes |
| --- | --- | --- |
| `colors` | `["#FFFFFF22"]` | Alternate by `(col + row)`. Alpha in the hex is how a texture stays a texture. |
| `size` | `10.0` | Cell edge in px. |
| `spacing` | `24.0` | Pitch, **clamped to `size`**: a smaller value would draw a solid sheet and lose the lattice. |
| `density` | `0.6` | Fraction of cells drawn. `1.0` fills every cell — required for a real checkerboard. |
| `density_ramp` | `"none"` | Where the field is densest. A ramp is what stops a scatter reading as noise. `edges` is a vignette — heavy at the border, clear through the middle, so the texture stays off whatever sits in the centre; `radial` is its exact inverse. |
| `radius` | `0.0` | `0` keeps the pixels hard-edged; anti-aliasing turns on above `0`. |
| `seed` | `7` | Occupancy is a hash of `(col, row, seed)`, so the pattern holds still across frames and is identical between two renders. |
| `motion` | `"none"` | `twinkle` fades cells on their own phase; `sweep` runs a band of extra density across the field. Scaled by the background's `speed`. |

> The lattice repeats on `spacing`, so it tiles seamlessly under a `world`
> view's camera pan.

The same `background` field also exists at the **view** level (`composition[].background`) — that's the recommended place for an ambient `halo` glow in a `world` view, since a per-scene shape glow either fails viewport validation or, once clipped to pass, becomes a visible hard-edged rectangle during a camera pan. See [rules/world-view.md](rules/world-view.md).

---

### Additional Style Fields

New style fields available on all components:

| Style field       | Type   | Default | Description                                              |
| ----------------- | ------ | ------- | -------------------------------------------------------- |
| `gradient-border` | object | `null`  | `{ "colors": ["#f00", "#00f"], "width": 2, "angle": 0 }` — gradient-colored border ring, border-radius aware, painted instead of `border` when both are set |

`stagger` and `timeline` are **root fields** (siblings of `style`), not style fields — see [rules/component-field-placement.md](rules/component-field-placement.md).

There is no `motion-path` *style* property — that name is a leftover from the pre-CSS `LayerStyle` model and was removed. To move a component along a path, use the **`motion_path` animation effect** (snake_case), which takes SVG path data and can orient the component along the tangent — see [rules/motion-path.md](rules/motion-path.md). The two spellings are one character apart and mean different things: `motion-path` in `style` is dropped, `motion_path` in `animation` works.

**Deprecated (accepted but never rendered — the validator warns):**

| Legacy field    | Use instead                                                  |
| --------------- | ------------------------------------------------------------ |
| `backdrop-blur` | `backdrop-filter: [{ "fn": "blur", "radius": N }]`            |
| `inner-shadow`  | `box-shadow: [{ ..., "inset": true }]`                        |

**Film grain (`noise` filter):** works in both `filter` and `backdrop-filter` chains. Deterministic — same `seed` produces identical grain on every frame.

```json
{ "backdrop-filter": [{ "fn": "blur", "radius": 24 }, { "fn": "noise", "intensity": 0.15, "seed": 42 }] }
```

---

### 3D Perspective Transforms

Any component can be rendered with true 3D perspective using keyframe animations on `rotate_x`, `rotate_y`, and `perspective` properties. The engine uses a Skia M44 4x4 matrix for real 3D rendering.

```json
{
  "type": "card",
  "position": { "x": 360, "y": 300 },
  "style": {
    "width": 1000,
    "height": 400,
    "background": "#FFFFFF08",
    "border-radius": 24,
    "backdrop-filter": [{ "fn": "blur", "radius": 15 }],
    "border": { "color": "#FFFFFF14", "width": 1 },
    "box-shadow": [{ "color": "#00000060", "offset-x": 0, "offset-y": 20, "blur": 60 }],
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

The `timeline` field — a **root field**, sibling of `style`, not nested inside it — allows defining sequential animation phases within a single scene. Each step triggers at a specific time and applies its own animation effects relative to that time.

```json
{
  "type": "card",
  "style": {
    "animation": [{ "name": "fade_in_up", "duration": 0.6 }]
  },
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

**40 presets (+ 6 char-only presets, text component only):**

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

# Validate a scenario (schema + geometry)
rustmotion validate -f scenario.json
rustmotion validate -f scenario.json --fix              # auto-fix safe overflows
rustmotion validate -f scenario.json --report r.json    # JSON report
rustmotion validate -f scenario.json --strict-anim      # per-frame check
rustmotion validate -f scenario.json --lenient          # warnings only

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
- [ ] No text uses `style.white-space: "nowrap"`/`"pre"` unless a finite `max-width` keeps it inside the viewport (use `marquee` for intentional bleeding) — see [rules/geometry-safety.md](rules/geometry-safety.md)
- [ ] Long codeblocks/terminals leave `auto_scroll` at its default (`true`) — never set `false` unless content is guaranteed to fit
- [ ] `rustmotion validate -f scenario.json` passes (zero schema **and** geometry violations) before presenting
