---
name: reverse
description: >
  Analyze any video file on disk and produce a structured rustmotion prompt —
  with named components, hex colors, estimated timings, and animation presets —
  ready to paste into Claude to generate a rustmotion JSON scenario.
  Trigger when: user provides a video path and asks to "analyze", "reverse",
  "recreate", or "understand" a video, or passes a file path to a video.
metadata:
  tags: video, analysis, reverse-engineering, rustmotion, vision
---

# Skill: Video → rustmotion Prompt

Transform any video file into a structured technical prompt for recreating it with rustmotion.

**Invocation:** `/reverse /path/to/video.mp4`

---

## Step 0 — Guard clauses

Run immediately before anything else:

```bash
which ffmpeg && which ffprobe || echo "MISSING"
```

If either is missing, stop and output:
```
ffmpeg non trouvé. Installer via : brew install ffmpeg
```

Verify the file path exists (use Bash `test -f`). If not found, stop with a clear error.

---

## Step 1 — Extract metadata

```bash
ffprobe -v quiet -print_format json -show_streams -show_format "/PATH/TO/VIDEO" 2>/dev/null
```

Extract from JSON output:
- `width`, `height` — from the video stream (`codec_type: "video"`)
- `duration` — from `format.duration` (float, seconds)
- `r_frame_rate` — parse fraction: `"30/1"` → 30, `"2997/100"` → round to 30

Display before proceeding:
```
Vidéo : 1920×1080 @ 30fps — 47.3s
```

---

## Step 2 — Adaptive frame sampling

Choose extraction rate based on duration:

| Duration | ffmpeg fps filter | Max frames |
|---|---|---|
| < 10s | `fps=2` | 20 |
| 10–30s | `fps=1` | 30 |
| 30–90s | `fps=0.5` | 45 |
| 90–300s | `fps=0.2` | 40 |
| > 300s | `fps=0.2` capped at 300s | 60 |

If duration > 300s, warn: `Vidéo > 5 min — analyse limitée aux 5 premières minutes`
Add `-t 300` to ffmpeg to enforce the cap.

Create temp directory and extract frames:
```bash
TMPDIR=$(mktemp -d /tmp/rustmotion_analyze_XXXX)

ffmpeg -i "/PATH/TO/VIDEO" \
  -vf "fps=RATE" \
  -q:v 2 \
  "$TMPDIR/frame_%04d.png" \
  -hide_banner -loglevel error
```

List extracted frames and their approximate timestamps:
```bash
ls -1 "$TMPDIR/frame_*.png" | sort
```

Frame N corresponds to time `(N-1) / RATE` seconds.

---

## Step 3 — Scene cut detection

```bash
ffmpeg -i "/PATH/TO/VIDEO" \
  -vf "select='gt(scene,0.35)',showinfo" \
  -f null - 2>&1 \
  | grep "pts_time" \
  | grep -oP 'pts_time:\K[0-9.]+'
```

Collect the timestamps. These are the scene boundaries.

If no cuts detected: treat the entire video as one scene.

Build a scene list with start/end/duration for each segment. Example:
```
Scène 1 : 0.0s → 3.2s (3.2s)
Scène 2 : 3.2s → 8.7s (5.5s)
Scène 3 : 8.7s → 12.0s (3.3s)
```

---

## Step 4 — Visual analysis (batched)

Use the Read tool to load PNG frames — **maximum 6 frames per batch** to avoid context overflow.

For each frame, record:
1. **Background** — solid color, gradient (direction + colors), image, pattern
2. **Layout** — number of visual zones, arrangement (centered / 2-col / fullscreen / etc.)
3. **Elements visible** — list everything: text (read the actual words), panels, icons, charts, images, UI controls
4. **Colors** — estimate hex for background, text, accents, card backgrounds
5. **Typography** — relative sizes (large headline ~72px / subtitle ~36px / body ~24px / caption ~18px)
6. **Delta from previous frame** — what appeared, disappeared, or moved

After each batch of frames, output a partial analysis:
```
Frames [N–M] (t=Xs–Ys) :
- Background : [description]
- Apparus : [list]
- Disparus : [list]
- Mouvement détecté : [yes/no + description]
- Couleurs : [hex list]
- Layout : [description]
```

---

## Step 5 — Animation inference

Compare consecutive frames to infer animations. Mark all inferences as estimated (`~`).

### Entrance presets

| Visual observation | rustmotion preset |
|---|---|
| Element fades in from transparent | `fade_in` |
| Element rises while fading in | `fade_in_up` |
| Element drops while fading in | `fade_in_down` |
| Element slides from left + fade | `fade_in_left` |
| Element slides from right + fade | `fade_in_right` |
| Element slides in from left (no fade) | `slide_in_left` |
| Element slides in from right | `slide_in_right` |
| Element slides in from bottom | `slide_in_up` |
| Element scales from small to full | `scale_in` |
| Element appears with 3D perspective tilt | `tilt_in` |
| Element bounces into place | `bounce_in` |
| Element starts blurry then sharpens | `blur_in` |
| Text reveals character by character | `typewriter` or `char_fade_in` |
| SVG border draws progressively | `draw_in` or `stroke_reveal` |
| Element flips on X axis | `flip_in_x` |
| Element flips on Y axis | `flip_in_y` |
| Element stretches elastically | `elastic_in` |
| Wipe reveal from left | `wipe_left` |
| Wipe reveal from right | `wipe_right` |

### Exit presets

| Visual observation | rustmotion preset |
|---|---|
| Element fades to transparent | `fade_out` |
| Element rises while fading out | `fade_out_up` |
| Element drops while fading out | `fade_out_down` |
| Element shrinks and disappears | `scale_out` |
| Element slides out left | `slide_out_left` |
| Element slides out right | `slide_out_right` |
| Element slides out upward | `slide_out_up` |
| Element slides out downward | `slide_out_down` |
| Element bounces out | `bounce_out` |
| Element blurs out | `blur_out` |
| Element rotates out | `rotate_out` |

### Continuous presets

| Visual observation | rustmotion preset |
|---|---|
| Gentle floating up/down loop | `float` |
| Gentle 3D floating | `float_3d` |
| Pulsing / breathing scale | `pulse` |
| Continuous rotation | `spin` |
| Shake / vibrate | `shake` |
| Glowing effect | `glow` |

### Timing estimation

- Count frames over which a transition occurs: `duration ≈ frame_count / fps`
- Start time = timestamp of first frame where change is visible
- Always prefix with `~`: `delay: ~0.4s`, `duration: ~0.6s`

---

## Step 6 — Component classification

Classify every visible element using these rustmotion component names:

**Text**
- `text` — any text content
- `gradient_text` — text with color gradient
- `rich_text` — mixed inline styling
- `caption` — small label or annotation

**Containers**
- `card` — panel with visible background, border-radius, shadow
- `div` / `container` — invisible layout wrapper (like HTML `<div>`)
- `flex` — horizontal or vertical flex layout container
- `grid` — CSS grid layout
- `positioned` — absolute-positioned container

**Media**
- `image` — photo or bitmap
- `icon` — icon (guess library: `lucide:name`, `ri:name`, `mdi:name`, `ph:name`)
- `svg` — SVG vector graphic
- `video` — embedded video clip
- `gif` — animated GIF

**Data visualization**
- `chart` — specify type: `bar`, `line`, `pie`, `donut`, `area`, `radar`, `scatter`, `horizontal_bar`, `stacked_bar`, `radial_bar`, `funnel`, `waterfall`
- `gauge` — semi-circular KPI gauge
- `sparkline` — inline mini-chart
- `stat` — KPI card (value + label + trend + optional sparkline)
- `progress` — linear or circular progress bar
- `counter` — animated number counter (standalone only, never inside card)
- `table` — data table
- `heatmap` — heatmap grid (GitHub-style)
- `treemap` — proportional rectangle chart
- `dot_map` — world map with data points

**UI controls**
- `badge` — small pill label with optional icon/dot
- `avatar` / `avatar_group` — profile images
- `notification` — toast notification (info/success/warning/error)
- `tooltip` — floating label with arrow
- `kbd` — keyboard key (3D style)
- `pill_nav` — tab bar with animated indicator
- `list` — bullet / numbered / checklist
- `stepper` — numbered step indicator
- `divider` — visual separator
- `callout` — highlighted callout bubble
- `rating` — star rating
- `switch` — animated toggle
- `slider` — horizontal slider
- `comparison` — before/after split view
- `countdown` — flip-clock countdown
- `marquee` — scrolling text ticker
- `skeleton` — loading placeholder
- `tag_cloud` — weighted word cloud

**Code & terminal**
- `codeblock` — syntax-highlighted code block
- `terminal` — macOS-style terminal window

**Diagrams**
- `arrow` — directional arrow
- `connector` — connecting line between elements
- `timeline` — vertical/horizontal timeline
- `line` — simple line

**Special**
- `mockup` — device frame (phone, browser, laptop)
- `qrcode` — QR code
- `particle` — particle field
- `shape` — geometric shape (`circle`, `rectangle`, `triangle`, `hexagon`, etc.)
- `lottie` — Lottie animation

**Default fallbacks:** use `card` for any decorated panel, `div` for invisible layout wrappers.

---

## Step 7 — Structured prompt output

Synthesize all batch analyses into the final prompt:

````markdown
# Analyse vidéo : [filename]

**Résolution source** : WIDTHxHEIGHT @ FPSfps
**Durée** : ~Xs
**Scènes détectées** : N

---

## Scène N (t=Xs – Ys | durée ~Zs)

**Background** : [ex: gradient radial `#0A0F1C` → `#1A2540`]
**Layout racine** : [ex: `flex` colonne centré, gap ~40px, padding ~80px]
**Résolution cible suggérée** : WIDTHxHEIGHT

### Composants

- `[type]` — [description courte]
  - Contenu : "[texte visible]" ou [description visuelle]
  - Style : `font-size` ~Npx · `color` #HEX · `font-weight` bold · `border-radius` ~Npx · `background` #HEX
  - Position : [ex: centré, ou `position: absolute` x~N y~N]
  - Animation entrée : `[preset]` · `delay` ~Xs · `duration` ~Ys *(estimé)*
  - Animation sortie : `[preset]` · `delay` ~Xs · `duration` ~Ys *(estimé, si détectée)*
  - Enfants :
    - `[type]` — [description]
      - Style : ...
      - Animation : ...

### Palette de couleurs

| Rôle | Hex |
|---|---|
| Background | #HEX |
| Texte principal | #HEX |
| Accent | #HEX |
| Card/panel | #HEX ou `rgba(R,G,B,A)` |

### Timing estimé

| Temps | Événement |
|---|---|
| T=~0.0s | [description] |
| T=~0.5s | [description] |
| T=~2.1s | Transition vers scène suivante |

---

[Repeat for each scene]

---

## Notes de reconstruction

- **Résolution recommandée** : WIDTHxHEIGHT (ratio source conservé)
- **FPS recommandé** : 30
- **Timings à affiner** : [list uncertain timings]
- **Composants ambigus** : [elements hard to classify]
- **Attention** : valeurs `~` sont estimées à ±0.3s — affiner après le premier rendu avec `rustmotion render --frame N`.
````

---

## Step 8 — Cleanup + optional save

Always clean up temp files after analysis:
```bash
rm -rf "$TMPDIR"
```

Then offer:
```
Sauvegarder le prompt dans [video_directory]/[video_name]_analysis.md ? [y/N]
```

If confirmed, write the prompt with the Write tool.

---

## Quality rules

- Use `~` on **every** estimated numeric value (timings, sizes, positions)
- Only output hex colors you actually observed — never invent
- When uncertain about a component: use `card` for decorated panels, `div` for layout
- Mark inference confidence explicitly: "animation d'entrée probable" vs "animation de sortie confirmée (disparition nette)"
- End with a clear "Notes de reconstruction" section so the user knows what to manually adjust
- The output is a **starting point**, not a 1:1 reconstruction — the Notes section must say so
