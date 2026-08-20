# Rule: Typography & Readability

Text that is too small, too light, or too low-contrast is invisible to the viewer. These rules define hard floors — not aesthetic preferences. Violating them produces unusable output regardless of whether the scene validates.

## Minimum font sizes per device

All values are **rustmotion render pixels** (absolute, not CSS). Cross-reference with [responsive-device-sizing.md](responsive-device-sizing.md) for the Tailwind equivalents.

| Role | Mobile 1080×1920 | Desktop 1920×1080 | Square 1080×1080 |
|---|---|---|---|
| **Title** | 90px | 45px | 75px |
| **Subtitle** | 60px | 30px | 50px |
| **Body** | 48px | 24px | 40px |
| **Label / badge** | 36px | 18px | 32px |
| **Caption** | 36px | 18px | 30px |

Below these values, text renders as an illegible smudge on the target device. These floors correspond to ~12pt minimum on-screen, the accepted minimum for body copy readability.

These are **floors**, not caps. The table above (and the desktop scale table in [responsive-device-sizing.md](responsive-device-sizing.md)) is sized for dashboard/UI-style content. It does not apply to the statement/display register below — see [1600-brutalist-style.md](1600-brutalist-style.md) for that register's own sizing.

## Statement register: hierarchy, scale, tracking

The reference films (1600.agency/Machina, Aikido-style dark-premium) use a much larger, flatter hierarchy than typical UI text, with a hard split between **display** type (huge, tight leading) and **labels** (small, normal leading) and almost nothing in between. Measured from the two shipped desktop (1920×1080) templates:

| Role | `examples/1600-style.json` (brutalist) | `examples/dark-premium.json` (dark-premium) |
|---|---|---|
| Hero / background numeral | 620–1100px | 320px |
| Statement / title lines | 118–300px | 64–168px |
| Kicker / label | 32–56px | 44–46px |
| `line-height` on display type | 0.80–1.02 | 1.04 (uniform) |
| `letter-spacing` on display type | `+1` to `+4` (big titles), `+4` to `+10` (small kickers) | `-2` (uniform, all sizes) |

The ratio between hero and label size is roughly **6×–20×** — far beyond a conventional type scale (Tailwind's `text-9xl` → `text-sm` ratio is ~9×, and its desktop pixel ceiling is 192px, well under this register's 300–1100px hero range). Do not reach for the Tailwind scale in [responsive-device-sizing.md](responsive-device-sizing.md) for this register; go straight to the sizes above or to [1600-brutalist-style.md](1600-brutalist-style.md).

### Tracking (letter-spacing) — two valid directions, not one

`style.letter-spacing` accepts negative values with no restriction (`Length::Px` is a plain signed `f32`, `css/units.rs:138-151`) — negative tracking is fully supported by the engine, not just tolerated.

- **Brutalist / uppercase grotesque** (1600.agency register): **positive** tracking, `+1`–`+2` on huge titles, `+4`–`+10` on small kickers (proportionally more open at small sizes). See [1600-brutalist-style.md](1600-brutalist-style.md).
- **Dark-premium / tight-leading statement** (Aikido-style register): **negative** tracking, a uniform `-2` applied at every display size from 64px to 320px, paired with the tight `1.04` line-height above. This is the register `dark-premium.json` uses throughout — negative tracking at scale is a legitimate, deliberate choice, not an oversight.

Pick one tracking direction per scenario and apply it consistently — don't mix positive and negative tracking across scenes in the same video.

## Line height

The correct range depends on register, and the two registers below are **both legitimate** — there is no engine-level floor on `line-height` (it is a plain `f32` multiplier with no validation, `css/style.rs:69`), so nothing stops either one from rendering correctly.

### Body / paragraph copy — 1.4 to 1.6

For multi-line **reading** text (body copy, descriptions, bullet lists) always set:

```json
{ "style": { "line-height": 1.5 } }
```

Acceptable range: `1.4` to `1.6`. Below `1.2`, wrapped paragraph lines visually collide at body sizes; above `2.0` it reads as too airy for content-dense scenes. Single-line headings and labels can omit `line-height` (the default is fine).

### Display / statement type — 0.80 to 1.05 (tight, deliberate)

For large **display** type (titles, hero numbers, stacked kinetic-typography lines — anything set at ~120px or above where lines are a designed block, not run-in prose), tight leading is the target, not a defect. This is the single most identifiable trait of the reference register (1600.agency / Machina, Aikido-style dark-premium): measured directly from the two shipped templates —

| Template | `line-height` | `font-size` range used at that leading |
|---|---|---|
| `examples/1600-style.json` | `0.80`–`1.02` | 118–1100px |
| `examples/dark-premium.json` | `1.04` (uniform) | 64–320px |

Use `0.92`–`1.02` as the default tight-leading range for stacked display lines; go as low as `0.80` for single giant background numerals/words where lines never wrap. See [1600-brutalist-style.md](1600-brutalist-style.md) for the full recipe.

**Precedence:** when a scene is authored in the brutalist/statement register, its display-type `line-height` follows this section (and [1600-brutalist-style.md](1600-brutalist-style.md)), not the 1.4–1.6 body-copy range above. Both ranges can appear in the same scenario — a hero title at `0.95` and a caption line at `1.5` are not in conflict; they're different roles.

## Contrast rules (hard rules, not guidelines)

### Rule C1: Dark background → white text

If the scene or card background starts with `#0`, `#1`, or `#2` in hex (i.e. dark):
- Text: `#FFFFFF` or near-white (`#E2E8F0`, `#CBD5E1`, `#F1F5F9`)
- **Never** use dark gray (`#334155`, `#475569`) or mid-gray text — insufficient contrast

### Rule C2: Light background → dark text

If the scene or card background is in the `#C` to `#F` range (light/white):
- Text: `#0F172A`, `#1E293B`, or `#334155`
- **Never** use white text on a light background

### Rule C3: Colored card backgrounds

| Card background color | Text color |
|---|---|
| Blue (`#3B82F6`, `#2563EB`) | `#FFFFFF` |
| Violet/purple (`#6366F1`, `#8B5CF6`) | `#FFFFFF` |
| Green (`#22C55E`, `#10B981`) | `#FFFFFF` |
| Red (`#EF4444`, `#DC2626`) | `#FFFFFF` |
| Yellow/amber (`#F59E0B`, `#FBBF24`) | `#0F172A` (dark) |
| Orange (`#F97316`) | `#0F172A` (dark) |

### Rule C4: Never use mid-gray as primary text

`#64748B`, `#94A3B8`, `#CBD5E1` are secondary/muted colors. They **fail** contrast on both dark and light backgrounds for body text. Use them only for truly secondary info (captions, metadata, placeholders).

### Rule C5: Gradient backgrounds

A gradient transitions through multiple lightness levels. Always verify contrast against **both** the darkest and lightest stop. When in doubt, use white text on dark gradients (`#0f172a` → `#1e1b4b`) and dark text on light gradients (`#F8FAFC` → `#E2E8F0`).

## Font weight

| Role | Weight |
|---|---|
| Title / H1 | `"bold"` (700) |
| Subtitle / H2 | `"bold"` or `600` |
| Body / paragraph | `"normal"` (400) |
| Counter | `"bold"` — animation looks better with heavier strokes |
| Badge label | `"bold"` — small text needs weight for visibility |
| Caption / metadata | `"normal"` or `500` |

## BAD: Font size below floor on mobile

```json
{
  "type": "text",
  "content": "Boost your productivity",
  "style": { "font-size": 36, "color": "#FFFFFF" }
}
```
36px ÷ 3 = 12px CSS → invisible on a phone screen. Below the 90px title floor.

## GOOD: Title at floor on mobile

```json
{
  "type": "text",
  "content": "Boost your productivity",
  "style": { "font-size": 108, "font-weight": "bold", "color": "#FFFFFF", "line-height": 1.4 }
}
```
108px ÷ 3 = 36px CSS → `text-4xl` → proper title. ✓

## BAD: White text on light background

```json
{
  "type": "card",
  "style": { "background": "#F8FAFC" },
  "children": [
    { "type": "text", "content": "Feature A", "style": { "color": "#FFFFFF", "font-size": 54 } }
  ]
}
```
White on `#F8FAFC` → invisible. Contrast ratio < 1.1:1.

## GOOD: Dark text on light card

```json
{
  "type": "card",
  "style": { "background": "#F8FAFC" },
  "children": [
    { "type": "text", "content": "Feature A", "style": { "color": "#0F172A", "font-size": 54 } }
  ]
}
```
`#0F172A` on `#F8FAFC` → contrast ratio > 15:1. ✓

## BAD: Low-contrast gray body text

```json
{ "type": "text", "content": "Description here", "style": { "color": "#64748B", "font-size": 48 } }
```
`#64748B` on `#0f172a` → contrast ratio ~3.8:1 (fails WCAG AA for body text).

## GOOD: Muted text for secondary info only

```json
{ "type": "text", "content": "Primary headline", "style": { "color": "#FFFFFF",   "font-size": 72, "font-weight": "bold" } },
{ "type": "text", "content": "Secondary note",  "style": { "color": "#94A3B8", "font-size": 42 } }
```
Gray only for secondary/muted info that doesn't need to be read at a glance. ✓
