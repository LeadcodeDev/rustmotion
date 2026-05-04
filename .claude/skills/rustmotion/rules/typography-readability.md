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

## Line height

For text that spans 2 or more lines, always set:

```json
{ "style": { "line-height": 1.5 } }
```

Acceptable range: `1.4` to `1.6`. Outside this range:
- `< 1.2` — lines collide, text becomes unreadable
- `> 2.0` — too airy for content-dense scenes

Single-line headings and labels can omit `line-height` (the default is fine).

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
