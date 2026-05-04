# Rule: Color Palettes & Consistency

Commit to a palette at the start of Phase 2 and never deviate across scenes. Random color choices create an incoherent, unprofessional result. Use one of the four pre-built palettes below, or derive a custom one following the same structure.

## The four standard palettes

Each palette defines every color needed for a complete video: scene background, card background, primary text, secondary text, accent, border, and divider.

### A. Dark Tech

Deep navy base with indigo/violet accent. Best for: SaaS products, developer tools, AI/ML, technical demos.

```
scene_bg:       #0f172a
card_bg:        #1E293B
card_bg_alt:    #0f172a   (darker cards for nesting)
accent:         #6366F1
accent_alt:     #8B5CF6
text_primary:   #FFFFFF
text_secondary: #94A3B8
card_border:    #FFFFFF14  (#FFFFFF with 8% alpha)
divider:        #FFFFFF1A
```

Gradient pair: `["#0f172a", "#1e1b4b"]` (dark navy to dark purple, 135°)

### B. Corporate Light

Clean white base with blue accent. Best for: B2B products, enterprise, presentations, reports.

```
scene_bg:       #F8FAFC
card_bg:        #FFFFFF
card_bg_alt:    #F1F5F9
accent:         #3B82F6
accent_alt:     #6366F1
text_primary:   #0F172A
text_secondary: #475569
card_border:    #E2E8F0
divider:        #CBD5E1
```

Gradient pair: `["#EEF2FF", "#F8FAFC"]` (light lavender to white, 135°)

### C. Playful / Colorful

Dark base with warm accent. Best for: consumer apps, social media, creative tools, energetic content.

```
scene_bg:       #0a0a14
card_bg:        #1a1a2e
card_bg_alt:    #16213E
accent:         #F59E0B
accent_alt:     #EC4899
text_primary:   #FFFFFF
text_secondary: #FDE68A
card_border:    #F59E0B33  (#F59E0B with 20% alpha)
divider:        #FFFFFF1A
```

Gradient pair: `["#0a0a14", "#1a0533"]` (dark navy to dark magenta, 135°)

### D. Minimal / Clean

Pure white base with near-black accent. Best for: documentation, editorial, premium brands.

```
scene_bg:       #FFFFFF
card_bg:        #F8FAFC
card_bg_alt:    #F1F5F9
accent:         #0F172A
accent_alt:     #475569
text_primary:   #0F172A
text_secondary: #64748B
card_border:    #E2E8F0
divider:        #E2E8F0
```

Gradient pair: `["#FFFFFF", "#F1F5F9"]` (white to light gray, 135°)

## Contrast rules (see also typography-readability.md)

These rules are derived from the palettes above and apply universally:

- **C1** — Scene backgrounds starting with `#0`, `#1`, `#2` → always white or near-white text
- **C2** — Scene backgrounds starting with `#E`, `#F` → always dark text (`#0F172A`)
- **C3** — Colored accent cards (blue, violet, green) → white text; yellow/amber cards → dark text
- **C4** — Never use `#64748B` or lighter grays as primary text color on any background

## Consistency rules

### Rule P1: One palette per video

Pick the palette in Phase 2. Every scene, every card, every text element uses colors from that palette. No exceptions.

### Rule P2: Use $ref for backgrounds

If the same gradient background appears in multiple scenes, define it once and reference it:

```json
{
  "backgrounds": {
    "dark_radial": {
      "type": "gradient",
      "gradient": {
        "type": "radial",
        "colors": ["#1e1b4b", "#0f172a"],
        "center_x": 0.5,
        "center_y": 0.4
      }
    }
  },
  "scenes": [
    { "background": { "$ref": "dark_radial" }, ... },
    { "background": { "$ref": "dark_radial" }, ... }
  ]
}
```

### Rule P3: Accent color as the single highlight

Use the accent color for: CTAs, icon fills, badge backgrounds, glow effects, border highlights.
Use `accent_alt` for secondary emphasis only — hover states, second-tier badges, decorative elements.
Never use two accent colors in the same scene at equal visual weight.

## BAD: Color drift between scenes

```
Scene 1: accent #6366F1 (indigo), card bg #1E293B
Scene 2: accent #3B82F6 (blue),   card bg #0D1117
Scene 3: accent #10B981 (green),  card bg #18181B
```
Three different accent colors, three different card backgrounds → no visual coherence.

## GOOD: Consistent palette

```
Scene 1: accent #6366F1, card bg #1E293B  (Dark Tech palette)
Scene 2: accent #6366F1, card bg #1E293B
Scene 3: accent #8B5CF6, card bg #1E293B  (accent_alt only for variety)
```

## BAD: Arbitrary text color not from palette

```json
{ "type": "text", "content": "Feature", "style": { "color": "#FF6B35", "font-size": 72 } }
```
`#FF6B35` is not in any palette, doesn't relate to the accent color, clashes.

## GOOD: Use text_secondary from palette for de-emphasis

```json
{ "type": "text", "content": "Feature",     "style": { "color": "#FFFFFF",  "font-size": 72, "font-weight": "bold" } },
{ "type": "text", "content": "Description", "style": { "color": "#94A3B8", "font-size": 48 } }
```
`#FFFFFF` (text_primary) + `#94A3B8` (text_secondary from Dark Tech) → coherent hierarchy. ✓

## How to present the palette in Phase 2

When proposing the scene plan, always include a palette line:

```
Palette: BG #0f172a | Text #FFFFFF | Accent #6366F1 | Cards #1E293B
```

This commits the colors before a single JSON line is written.
