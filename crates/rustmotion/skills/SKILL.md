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
| `<div>`, plain or decorated | `{"type":"div"}` — flex par défaut; ajoute `background`/`border-radius`/`box-shadow` dans `style` pour un panneau décoré, ou laisse-les absents pour un groupement pur. `card`/`flex`/`grid`/`positioned`/`container` sont des alias historiques du même type — aucune différence de comportement, `div` est la forme canonique. |
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

## Composition over cataloguing

Fifty-three component types exist, but three different things hide behind that one number:

1. **Algorithms** (`dot_map`, `treemap`, `lottie`, `video`, `gif`, `qr_code`, `waveform`/`audio_spectrum`, `image`) render something a JSON tree of shapes and text genuinely cannot — a land bitmap, an FFT, Reed-Solomon error correction, recursive slice-and-dice. Reach for these directly. (`codeblock` used to belong here for syntax highlighting; it was deleted outright, not deprecated — the tokenising didn't need to live in the engine, since whoever writes the scenario is a language model that already knows the grammars and can emit `rich_text` with a coloured span per token directly. See [rules/composition-recipes.md](rules/composition-recipes.md) for the recipe.)
2. **Primitives** (`text`, `rich_text`, `gradient_text`, `shape`, `svg`, `icon`, `line`, `arrow`, `connector`, `div`, `cursor`, `pointer`) are the alphabet. Everything else is built from these.
3. **Composite UI widgets** (`stat`, `badge`, `gauge`, `sparkline`, `progress`, `counter`, `number_wheel`, `kbd`, `tooltip`, `list`, `stepper`, `comparison`, `countdown`, `pill_nav`, `avatar`, `avatar_group`, `rating`, `switch`, `slider`, `skeleton`, `tag_cloud`, `callout`, `divider`, `success_check`, `timeline`, `marquee`, `chart`, `heatmap`, `table`, `particle`, `caption`, `mockup`) are frozen arrangements of primitives — a `div` + `text` + `shape` + an animation, baked into a single JSON type with its own field names. They **still exist and still render byte-identically** — nothing here changes what a scenario produces. Issue #333 phase B put a Rust-level `#[deprecated]` attribute on 16 of the original 27 struct definitions (the other 11 carry the same message as a doc comment instead, because a struct-level `#[deprecated]` also deprecates field *reads*, and a few of these are read directly by CLI-internal code outside this crate — see each type's own note in `crates/rustmotion-components/src/*.rs`). That attribute is a signal for a Rust contributor writing `Badge { .. }`/`Stat { .. }`/etc. by hand in this codebase or a downstream crate; it never fires for JSON scenario authoring, which goes through this crate's own generated deserializer. They are simply no longer documented here, and no longer the reflex to reach for. (`notification` used to be a member of this class too; like `codeblock`, it was deleted outright rather than merely deprecated — see the `div`-sliding-toast recipe in [rules/composition-recipes.md](rules/composition-recipes.md).)

   `chart`, `heatmap`, `table`, `particle`, `caption`, and `mockup` joined this class later, once this chantier's `for-each` gained arithmetic expressions, deterministic `rand(seed, i)`, computed path data, and node references — the exact machinery that makes a five-bar chart a `for-each` over five items with one `height` expression instead of a dedicated component. All six carry the real `#[deprecated]` struct attribute (see [rules/composition-recipes.md](rules/composition-recipes.md) for what composes each), with a narrow `#[allow(deprecated)]` on the handful of CLI call sites that read `caption`/`mockup` fields directly (`crates/rustmotion/src/cli/commands/{geometry,validate_schema,info}.rs`) rather than a blanket one. (`terminal` was also in this later-joining group; it was deleted outright, not deprecated — see the `div`-title-bar-plus-`text` recipe in [rules/composition-recipes.md](rules/composition-recipes.md).)

**Why:** a shipped widget library is still a default that anchors a generator toward filling in blanks (`stat` with a value and a label) instead of designing the actual layout the brief calls for. Art direction is per video; a component defined *inside* the scenario and instantiated with `components` + `for-each` gives the same reuse without importing someone else's opinion about what a KPI card looks like. An example teaches composition; a library teaches filling in blanks.

**The decision, recorded:** nothing is added to the frozen-widget class from here on. A generator that needs a stat card, a progress bar, a stepper, or any other shape a UI kit would hand over **defines it in the scenario** with `div`/`text`/`shape`, and reaches for `components` + `for-each` the moment more than one instance is needed. See [rules/composition-recipes.md](rules/composition-recipes.md) — read it before reaching for a component not in the catalog below — and the worked examples under `examples/composition-*.json`.

If a subject genuinely needs one of the 32 frozen widgets by name (they are still valid JSON, still render, and are exercised in `examples/component-showcase.json` and `examples/mega-showcase.json`), using it is not an error. The point is that a generator should no longer see them first and reach for them by default.

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
| Stats / numbers | a KPI `card` (`div`/`card` + `text` + `shape`) defined once as a `components` entry, instantiated with `for-each` — see `examples/composition-kpi-row.json` |
| Features / benefits | `card` grid + `icon`, one `components` entry per card instantiated with `for-each` |
| Code / technical | `rich_text` with a coloured span per token for the code, plus a terminal-style `div` (title bar + monospace `text` lines under `typewriter`) — see [rules/composition-recipes.md](rules/composition-recipes.md) |
| Process / steps | `connector`/`line` + `text` labels, one step defined as a `components` entry and repeated with `for-each` — see [rules/composition-recipes.md](rules/composition-recipes.md) |
| Comparison | `flex` row with 2 `card` side by side |
| Testimonial | `card` with `shape` circle (avatar) + `text` italic |
| Pricing | `card` with `text` + `shape` (see the KPI card pattern above — the number is static, not counted up) |
| Partner logos | `flex` row + `icon` (simple-icons:xxx) |
| CTA / call to action | a pill (`div`/`shape` + `icon` + `text`) + glow + confetti (`for-each` + `rand`/`sin` drift — see [rules/composition-recipes.md](rules/composition-recipes.md)) |
| Hero / intro | `text` with `char_scale_in` + main `icon` (hero role: 160-200px mobile / 80-100px desktop) |
| Transition / ambiance | confetti/stars (`for-each` + `rand`/`sin` drift) + `animated-background` |
| Grouped transforms | `div` wrapping children + shared `timeline` scale/fade |

The user validates or adjusts the plan before proceeding.

### Phase 3: Iterative scene-by-scene construction

**Pre-generation checklist** — verify before writing each scene's JSON:
0. **HTML/CSS mental model** — sketch the layout mentally as HTML divs before writing JSON. Every element should have a reason to be in flow OR absolute. If you're reaching for `x`/`y` on a main content element, stop and restructure: use `gap` for spacing between siblings, `padding` for inner spacing, `align-items`/`justify-content` for centering, `flex-grow` for elastic elements. `x`/`y` is reserved for decorative blobs and overlays only. See [rules/html-css-mental-model.md](rules/html-css-mental-model.md).
1. All font sizes meet the floor for the target device (see [rules/typography-readability.md](rules/typography-readability.md))
2. `start_at + delay + duration ≤ scene_duration` for every animation (see [rules/animation-completion-budget.md](rules/animation-completion-budget.md))
3. Text color contrasts correctly with the scene/card background (dark bg → white text, light bg → dark text)
4. Scene duration ≥ reading time of all text (`word_count ÷ 2.5`) (see [rules/scene-pacing.md](rules/scene-pacing.md))
5. If dynamism level ≥ 2: at least one non-text element per scene has a continuous effect (`float_3d`/`wiggle`/`orbit` with `loop: true`). Never apply continuous motion to primary text. See [rules/dynamic-depth.md](rules/dynamic-depth.md).

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
- [rules/halo-shapes.md](rules/halo-shapes.md) - `halo` beyond circles: `radius_x`/`radius_y`/`rotation` for a wide thin band of light, and why the blur follows the short axis
- [rules/zoom-blur-transition.md](rules/zoom-blur-transition.md) - The radial "tunnel" cut: `zoom_blur`'s `strength`/`origin`, why it had to be a transition and not an effect, and the pivot-coincident-edge trap
- [rules/chromatic-aberration.md](rules/chromatic-aberration.md) - Per-element red/cyan fringe on arrival: `chromatic_aberration`'s `amount`, how its curve differs from `chromatic_wipe`'s, and the `amount`-not-`amplitude` trap
- [rules/shatter.md](rules/shatter.md) - `shatter`: the node's own render broken into deterministic Voronoi shards that fly apart — the three modes, the fraction-not-pixels `origin`, and why outside its window it is a different code path, not a progress of zero
- [rules/burst.md](rules/burst.md) - `burst`: the ring of strokes a badge throws when it pops — the head-out/tail-follows stroke, why its zero-at-both-ends is geometric and not only a window test, and why `gap` never lets it touch the box
- [rules/inflated-material.md](rules/inflated-material.md) - `material: "inflated"`: shading derived from the clipped silhouette, so each branch of a star gets its own relief — and why `bevel` must stay small relative to the shape
- [rules/material-and-light.md](rules/material-and-light.md) - Lit surfaces: `style.material`'s three presets, the scene-wide `light` that makes them agree, and why the material follows the box and not a `shape`'s own geometry
- [rules/emitter-lifecycle.md](rules/emitter-lifecycle.md) - `emitter`: a particle field whose lifecycle is closed-form, so `still --time` and a full render agree — and why there is no particle-count field
- [rules/layout-surface.md](rules/layout-surface.md) - Project a flat grid onto a cylinder or sphere with one shared vanishing point — and why children still paint in declaration order
- [rules/dot-map-orthographic.md](rules/dot-map-orthographic.md) - `dot_map` as a globe: orthographic projection, far-hemisphere culling, great-circle arcs and limb shading
- [rules/camera-3d.md](rules/camera-3d.md) - Tilt a whole shot with `camera.rotate_x`/`rotate_y`/`perspective`: one shared vanishing point, and rotation scaled by each plane's `style.depth`
- [rules/depth-of-field.md](rules/depth-of-field.md) - Defocus by plane: `camera.focus`/`aperture` on the `style.depth` scale, rack focus by keyframe, and why nothing moves without distinct depths
- [rules/char-animation-rich-and-gradient-text.md](rules/char-animation-rich-and-gradient-text.md) - `char_*` presets on `rich_text` and `gradient_text`: stagger across span boundaries, per-span `ink_from`, and why `ink_from` is inert on a gradient
- [rules/text-morph.md](rules/text-morph.md) - `text.morph`: matched letters travel, unmatched ones fade or scramble — how it differs from `text.states` + `swap`
- [rules/text-component-parity.md](rules/text-component-parity.md) - Where `text`, `rich_text` and `gradient_text` disagreed: colour alpha, literal whitespace and baseline, the CSS angle convention and explicit `stops`
- [rules/mask-transition.md](rules/mask-transition.md) - `mask` and `blob`: reveal through any silhouette, and an organic wobbling edge whose covering radius solves itself
- [rules/feathered-wipes.md](rules/feathered-wipes.md) - `feather` and `band_color`, shared by every `wipe_*` plus `mask` and `blob`
- [rules/animatable-spacing-and-trim.md](rules/animatable-spacing-and-trim.md) - `letter_spacing` and `draw_start` as keyframe properties: why animating spacing overflows its box, and how `draw_start` differs from the `shape` field of the same name
- [rules/shape-draw-start-and-path-morph.md](rules/shape-draw-start-and-path-morph.md) - `shape.draw_start` and `shape.path_morph` — and why they are component fields, not `style.animation` keyframe properties
- [rules/iris-transition.md](rules/iris-transition.md) - `iris` beyond a centred circle: `origin`, `shape`, `fill`+`hold`, `ring` and `reverse`, and the pill coverage approximation
- [rules/whip-transition.md](rules/whip-transition.md) - The whip cut: a directional slide that streaks **both** frames along its axis, unlike `zoom_blur` which streaks only the outgoing one
- [rules/draw-progress-stroke.md](rules/draw-progress-stroke.md) - `draw_progress` at 0 paints nothing, and an `svg` draw-on matches the finished mark's stroke width, cap and join
- [rules/svg-text-fonts.md](rules/svg-text-fonts.md) - Why `<text>` inside an `svg` needs a resolvable font face, and what happens when the host has none
- [rules/vhs-tear.md](rules/vhs-tear.md) - The `vhs` scene effect: bands slid sideways, noise, scanlines and a travelling tracking line — bounded by `at`/`duration` because it is a beat, not a filter
- [rules/rich-text-pills.md](rules/rich-text-pills.md) - Pill spans in `rich_text`: padding that moves the following span, `box-decoration-break: clone` on a wrap, and rotation that turns the box without touching layout
- [rules/motion-blur-and-trail.md](rules/motion-blur-and-trail.md) - `motion_blur` and `trail`: ghosts never take a flex slot, they carry their node's children, and `mode: "smear"` replaces them with a displacement blur
- [rules/directional-blur.md](rules/directional-blur.md) - Blur on one axis: `radius-x`/`radius-y`, the `directional-blur` filter, and the animatable `blur_x`/`blur_y`
- [rules/geometry-safety.md](rules/geometry-safety.md) - Keep all content inside the viewport: `white-space`, `auto_scroll`, `overflow` semantics + violation kinds
- [rules/border-style.md](rules/border-style.md) - `dashed`, `dotted` and `double` borders, their cadence formulas, and animating `border-radius` by keyframe
- [rules/clip-path-morph.md](rules/clip-path-morph.md) - Animating a `clip-path` via `kind: "morph"` and the scalar `clip_path_progress`, what interpolates, and what a kind mismatch does
- [rules/clip-path.md](rules/clip-path.md) - Non-rectangular masking: the six `clip-path` shapes, how their percentages resolve, and why `node-path` is not one of them yet
- [rules/overlapping-scenes.md](rules/overlapping-scenes.md) - Make an element outlive a cut: overlapping `at` windows composite instead of replacing, who supplies the background, and why `snap` never creates an overlap
- [rules/even-dimensions.md](rules/even-dimensions.md) - Use even width/height for H.264 encoding
- [rules/composition-recipes.md](rules/composition-recipes.md) - **Read this before reaching for a UI-widget component.** Composing KPI cards, pill rows, progress bars, and other former "frozen composition" shapes from primitives, `components`, and `for-each`
- [rules/templates-and-iteration.md](rules/templates-and-iteration.md) - `for-each`/`components`/`use` mechanics: bindings, param defaults, ordering of passes, named errors
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
- [rules/dot-map-coordinates.md](rules/dot-map-coordinates.md) - Dot map: use real lat/lng coordinates, common city reference table

### Design quality (nouvelles règles)

- [rules/animation-completion-budget.md](rules/animation-completion-budget.md) - **CRITICAL:** Animation budget formula — every animation must complete within its scene duration
- [rules/animations-compose.md](rules/animations-compose.md) - Two effects on one property combine — a product for `opacity`/`scale`, a sum for `translate`/`rotation`, last-written for the rest — and why a resting value of `-1` is what tells `not animated` apart from `animated to zero`
- [rules/typography-readability.md](rules/typography-readability.md) - **CRITICAL:** Minimum font sizes per device/role, line-height rules, contrast hard rules
- [rules/scene-pacing.md](rules/scene-pacing.md) - Scene duration formula (reading time + animation budget), density limits, dense/breathing alternation
- [rules/color-palettes.md](rules/color-palettes.md) - 4 ready-to-use palettes (Dark Tech / Corporate / Playful / Minimal), consistency rules
- [rules/icon-sizing-hierarchy.md](rules/icon-sizing-hierarchy.md) - Icon sizing (hero/card/inline roles), card spacing minimums, row layout by device
- [rules/depth-layering.md](rules/depth-layering.md) - **NEW:** Visual depth — 3 planes (bg/mid/fg), z-index, blur, shadow hierarchy, scale gradient, 3D tilt
- [rules/dynamic-depth.md](rules/dynamic-depth.md) - **NEW:** Multi-element parallax — wiggle seeds, float_3d preset, camera zoom, orbit phases, frequency hierarchy
- [rules/component-field-placement.md](rules/component-field-placement.md) - **CRITICAL:** Field placement (root vs style) — `width`/`height`/`animation` inside `style`; `fill`/`stroke`/`timeline`/`stagger` at root; `box-shadow` as array; silently-dropped component pitfalls
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
| `examples/mega-showcase.json` | 1920×1080 | 9 | Largest example — grid layout, timeline component, most component types in one file (the catalogue-tour file; not a composition model — see below) |
| `examples/composition-kpi-row.json` | 1920×1080 | 1 | A `stat`-style KPI card built from `card`+`icon`+`shape`+`text`, defined once as a `components` entry and instantiated four times with `for-each` |
| `examples/composition-pill-row.json` | 1920×1080 | 1 | A `badge`-style pill built from `div`+`icon`+`text`, repeated with `for-each` |
| `examples/composition-progress-bars.json` | 1920×1080 | 1 | A `progress`-style bar from two primitives (a `card` track + an animated-width `shape` fill), four instances via `for-each` |
| `examples/composition-step-flow.json` | 1920×1080 | 1 | A `stepper`-style flow from `card`+`text`+`shape` connectors, each `for-each` item emitting a sibling pair (node + connector) |

These four are the worked reference for [rules/composition-recipes.md](rules/composition-recipes.md) — read that file first when a brief calls for something that used to be one of the frozen UI-widget components.

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
          "type": "div",
          "style": {
            "flex-direction": "column",
            "background": "#1a1b26",
            "border-radius": 16,
            "overflow": "hidden",
            "width": 1400,
            "height": 400,
            "animation": [{ "name": "fade_in_up", "delay": 0.3, "duration": 0.5 }]
          },
          "children": [
            {
              "type": "div",
              "style": { "flex-direction": "row", "align-items": "center", "gap": 8, "padding": { "top": 10, "right": 14, "bottom": 10, "left": 14 }, "background": "#16161e" },
              "children": [
                { "type": "shape", "shape": "circle", "fill": "#ff5f56", "style": { "width": 12, "height": 12 } },
                { "type": "shape", "shape": "circle", "fill": "#ffbd2e", "style": { "width": 12, "height": 12 } },
                { "type": "shape", "shape": "circle", "fill": "#27c93f", "style": { "width": 12, "height": 12 } },
                { "type": "text", "content": "src/main.rs", "style": { "font-size": 13, "color": "#8b949e", "margin": { "left": 8 } } }
              ]
            },
            {
              "type": "div",
              "stagger": 0.5,
              "style": { "flex-direction": "column", "padding": 24, "gap": 4 },
              "children": [
                {
                  "type": "rich_text",
                  "spans": [
                    { "text": "fn ", "color": "#bb9af7" },
                    { "text": "main", "color": "#7aa2f7" },
                    { "text": "() {", "color": "#c0caf5" }
                  ],
                  "style": { "font-family": "JetBrains Mono", "font-size": 22, "white-space": "pre", "animation": [{ "name": "typewriter", "duration": 0.5 }] }
                },
                {
                  "type": "rich_text",
                  "spans": [{ "text": "    println!(\"Hello, world!\");", "color": "#c0caf5" }],
                  "style": { "font-family": "JetBrains Mono", "font-size": 22, "white-space": "pre", "animation": [{ "name": "typewriter", "duration": 0.5 }] }
                },
                {
                  "type": "rich_text",
                  "spans": [{ "text": "}", "color": "#c0caf5" }],
                  "style": { "font-family": "JetBrains Mono", "font-size": 22, "white-space": "pre", "animation": [{ "name": "typewriter", "duration": 0.5 }] }
                }
              ]
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
| `codec`      | string | `null`      | `"h264"` (8-bit), `"h264_10bit"`, `"h265"`, `"vp9"`, `"prores"` |
| `crf`        | u8     | `23`        | Constant Rate Factor (0-51, lower = better quality)     |

> **Encoding note:** H.264 outputs 8-bit (`yuv420p`) by default, which plays in QuickTime and Safari. `--codec h264_10bit` gives `yuv420p10le` and reduces banding on dark gradients, at the cost of those two players refusing the file. For best quality on gradient-heavy videos, use `--codec prores` (lossless).

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

**`layout` options:** `direction` (column/row), `gap`, `align_items` (start/center/end/stretch), `justify_content` (start/center/end/space_between/space_around/space_evenly), `padding` (f32 only — unlike `style.padding`, `layout.padding` does not accept the `{top,right,bottom,left}` object form)

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

The engine has **53** component types total (`Component` enum, `crates/rustmotion-components/src/lib.rs`). Two of the three classes get a dedicated write-up below:

- **Algorithms** — cannot be composed from drawing primitives, so they stay as first-class components: `qr_code`, `dot_map`, `treemap`, `lottie`, `video`, `gif`, `waveform`/`audio_spectrum` (see [rules/audio-reactive.md](rules/audio-reactive.md)), `image`. Nine total. (`codeblock` used to be here for syntax highlighting; deleted outright — see [rules/composition-recipes.md](rules/composition-recipes.md) for the `rich_text`-per-token recipe that replaces it.)
- **Primitives** — `text`, `rich_text`, `gradient_text`, `shape`, `svg`, `icon`, `line`, `arrow`, `connector`, `div`, `cursor`, `pointer` (see [rules/pointer-walkthrough.md](rules/pointer-walkthrough.md)). The container (`div` — `card`/`flex`/`grid`/`positioned`/`container` are the same type, kept as JSON aliases) is covered in the "Mental Model: Think HTML/CSS" section above. These are the building blocks for everything else — see [rules/composition-recipes.md](rules/composition-recipes.md).

The **third class — composite UI widgets** (`stat`, `badge`, `gauge`, `sparkline`, `progress`, `counter`, `number_wheel`, `kbd`, `tooltip`, `list`, `stepper`, `comparison`, `countdown`, `pill_nav`, `avatar`, `avatar_group`, `rating`, `switch`, `slider`, `skeleton`, `tag_cloud`, `callout`, `divider`, `success_check`, `timeline`, `marquee`, `chart`, `heatmap`, `table`, `particle`, `caption`, `mockup`) still exists in the engine and still renders byte-identically — nothing described here is removed, and the JSON you write for a scenario is unaffected. Since issue #333 phase B, most of them carry a Rust-level `#[deprecated]` marker at their struct definition, naming the primitive recipe that replaces them (a signal for Rust code, not for scenario JSON); `chart`/`heatmap`/`table`/`particle`/`caption`/`mockup` joined the same class once `for-each` gained the arithmetic/rand/computed-path machinery to reproduce them (see [rules/composition-recipes.md](rules/composition-recipes.md)). It is intentionally **not catalogued below**. See "Composition over cataloguing" near the top of this document for why, and [rules/composition-recipes.md](rules/composition-recipes.md) for how to get the same result from primitives. (`notification` and `terminal` used to be members of this class; both were deleted outright rather than deprecated — see [rules/composition-recipes.md](rules/composition-recipes.md) for the `div`-based recipes that replace them.)

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

### `text`

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
| `clip-path`       | object   | Non-rectangular mask on the element itself, background/border/outer-shadow included. `kind`: `inset`, `circle`, `ellipse`, `polygon`, `path`, `none`. A chamfered frame is an eight-point `polygon`. `kind: node-path` is declared but **not implemented** and warns on stderr. See [rules/clip-path.md](rules/clip-path.md). |

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

### `shape`

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

### `image`

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

### `svg`

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
| `reveal`   | enum     | `"stroke"` — how a draw-on animation uncovers the artwork: `"stroke"` traces each path as a contour, `"fill"` sweeps a mask across the painted shape so gradients and patterns show as they arrive |
| `draw`     | bool     | `false` — force draw-on mode even at `draw_progress: 1.0`    |
| `draw_stroke_width` | f32 | `2.0` — stroke width used when tracing a fill-only path   |

Style: `width`, `height` (default: intrinsic SVG dimensions)

Drive either mode with the `draw_progress` animatable property. The two compose: stack a
`reveal: "stroke"` copy that fades out over a `reveal: "fill"` copy that fades in, and the mark
draws its outline first, then takes its colour.

```json
{ "type": "svg", "src": "logo.svg", "reveal": "fill",
  "style": { "width": 260, "height": 281, "animation": [
    { "name": "keyframes", "duration": 2.2, "keyframes": [
      { "property": "draw_progress", "easing": "ease_in_out",
        "keyframes": [{ "time": 0, "value": 0 }, { "time": 2.2, "value": 1 }] }] }] } }
```

### `icon`

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

### `video`

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

### `gif`

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

### `div`

The one container type: a box with CSS-like flex & grid layout that lays out `children`. Decoration (`background`, `border-radius`, `border`, `box-shadow`) is entirely opt-in through `style` — set none of them for an invisible grouping wrapper (HTML `<div>`), or set them for a visually decorated panel. There is no separate "decorated" type: the box is the same either way, only `style` differs.

`card`, `flex`, `grid`, `positioned`, and `container` are accepted as JSON aliases for `"type": "div"` — old scenarios using any of them keep working, and they render byte-identically to `div`, because they deserialize into the exact same component. Write new scenarios as `div`.

Each dimension (`width`/`height` in `style`) can be a number or `"auto"`.

**Flex example** (default `display`):
```json
{
  "type": "div",
  "style": { "width": 800, "height": 100, "flex-direction": "row", "gap": 16 },
  "children": [
    { "type": "shape", "shape": "rect", "fill": "#FF0000", "style": { "width": 100, "height": 100 } },
    { "type": "shape", "shape": "rect", "fill": "#00FF00", "style": { "width": 100, "height": 100, "flex-grow": 1 } },
    { "type": "shape", "shape": "rect", "fill": "#0000FF", "style": { "width": 100, "height": 100 } }
  ]
}
```

**Decorated panel** (same type, `style` adds a background/border-radius):
```json
{
  "type": "div",
  "style": {
    "width": 800,
    "height": "auto",
    "flex-direction": "row",
    "align-items": "center",
    "gap": 16,
    "padding": 24,
    "background": "#1E293B",
    "border-radius": 16
  },
  "children": [
    { "type": "icon", "icon": "lucide:check-circle", "style": { "width": 48, "height": 48, "color": "#22C55E" } },
    { "type": "text", "content": "Feature enabled", "style": { "font-size": 32, "color": "#FFFFFF" } }
  ]
}
```

**Grid example (2x2):** Note: grid containers need explicit `height` (not `"auto"`) — see [rules/grid-card-height.md](rules/grid-card-height.md). `grid-template-columns`/`grid-template-rows` is `Vec<GridTrack>`, an **untagged** enum: a bare number means px, a quoted string like `"1fr"` carries the unit, `"auto"` is the keyword. The object forms `{"fr": N}` / `{"px": N}` shown in older docs do **not** match any variant and drop the whole component.
```json
{
  "type": "div",
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

**Absolute positioning of children:** any container's children can carry `position: {x, y}` — it's a property of the child, not a special container mode. Give the container a transparent background and explicit size, and each `position`-ed child is placed relative to its top-left; children without `position` still lay out with flex/grid.
```json
{
  "type": "div",
  "style": { "width": 1920, "height": 1080, "background": "#00000000", "padding": 0 },
  "children": [
    { "type": "shape", "shape": "rect", "fill": "#1E293B", "position": { "x": 0, "y": 0 }, "style": { "width": 400, "height": 300, "border-radius": 16 } },
    { "type": "icon", "icon": "lucide:phone-off", "position": { "x": 170, "y": 120 }, "style": { "width": 64, "height": 64, "color": "#FFFFFF" } }
  ]
}
```

**Style fields:**

| Style field              | Type        | Default    |
| ------------------------ | ----------- | ---------- |
| `display`                | enum        | `"flex"` — `"flex"` or `"grid"` |
| `background`             | string      | `null`     |
| `border-radius`          | f32         | `null` — sharp corners; `card`/`flex`/etc. are the same component as `div` and carry no special default either |
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

`timeline` and `stagger` are **root fields**, not `style` — `CssStyle` has no `timeline` key and `deny_unknown_fields` drops the whole component if you nest it there.

### `arrow`

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

### `connector`

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

### `lottie`

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

### `cursor`

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

### `line`

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

### `rich_text`

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

### `gradient_text`

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

### `treemap`

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

### `dot_map`

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

### `qr_code`

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

**Animatable properties:** `opacity`, `translate_x`, `translate_y`, `scale.x`, `scale.y`, `scale` (both axes), `rotation`, `blur`, `color`, `rotate_x`, `rotate_y`, `perspective`

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
| Char (text, rich_text, gradient_text) | `char_scale_in`, `char_fade_in`, `char_wave`, `char_bounce`, `char_rotate_in`, `char_slide_up` (per-char/word animation, extra fields: `stagger`, `granularity`, `overshoot`) |

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
rustmotion render -f scenario.json -o output.mp4

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
rustmotion info -f scenario.json

# Render a single frame (0-indexed) as PNG
rustmotion render -f scenario.json -o frame.png --frame 0

# Render with specific codec/format
rustmotion render -f scenario.json -o output.webm --codec vp9 --format webm

# Render as GIF
rustmotion render -f scenario.json -o output.gif --format gif

# Render as PNG sequence
rustmotion render -f scenario.json -o frames/ --format png-seq

# Machine-readable output
rustmotion render -f scenario.json -o output.mp4 --output-format json
```

`render` and `info` only accept the path via `-f`/`--file` — there is no positional form; passing a bare path errors with `unexpected argument`.

---

### Pre-Delivery Checklist

Before presenting a generated scenario to the user, verify:

- [ ] **ZERO `rgba()` / `rgb()` values** — all colors in `#RRGGBB` or `#RRGGBBAA` hex format
- [ ] All scenes have `"layout": {"align_items": "center", "justify_content": "center"}` for centered composition
- [ ] `concentric_circles` animated-background on at least 4 scenes for visual depth
- [ ] No `end_at` on counters (makes them disappear — use `start_at` only)
- [ ] No text uses `style.white-space: "nowrap"`/`"pre"` unless a finite `max-width` keeps it inside the viewport (use `marquee` for intentional bleeding) — see [rules/geometry-safety.md](rules/geometry-safety.md)
- [ ] `rustmotion validate -f scenario.json` passes (zero schema **and** geometry violations) before presenting
