# Rule: Responsive Device Sizing (Tailwind 4 based)

## Description
When a user requests a video for a specific device class (mobile, tablet, desktop), all component dimensions must be scaled appropriately. rustmotion pixel values are **absolute render pixels**, NOT CSS logical pixels. A phone screen renders 1080px across ~375 CSS points — so everything is at ~3x density. A 48px font renders as ~16pt on screen, which is body text size, not a headline.

## Conversion formula

The scaling factor from rustmotion pixels to perceived CSS-like size:
- **Mobile 1080×1920**: divide by 3 to get equivalent CSS px (e.g., 96px render = 32px CSS = `text-3xl`)
- **Desktop 1920×1080**: divide by ~1.5 (e.g., 48px render = 32px CSS = `text-3xl`)
- **Square 1080×1080**: divide by ~2.5

**Always think in Tailwind 4 type scale first, then multiply by the device factor.**

## Device aliases

| User says | Device class | Resolution |
|---|---|---|
| "mobile", "phone", "portrait", "story", "reel", "TikTok" | **Mobile 9:16** | 1080×1920 |
| "desktop", "landscape", "YouTube", "presentation" | **Desktop 16:9** | 1920×1080 |
| "tablet", "iPad" | **Tablet** | 1080×1440 or 1200×1600 |
| "square", "Instagram", "LinkedIn" | **Square 1:1** | 1080×1080 |

## Tailwind 4 type scale → rustmotion pixels

Reference: Tailwind CSS default font sizes.

| Tailwind class | CSS size | Mobile (×3) | Desktop (×1.5) | Square (×2.5) | Usage |
|---|---|---|---|---|---|
| `text-sm` | 14px | 42 | 21 | 35 | Micro labels |
| `text-base` | 16px | 48 | 24 | 40 | Small / label text |
| `text-lg` | 18px | 54 | 27 | 45 | Card labels |
| `text-xl` | 20px | 60 | 30 | 50 | Body text |
| `text-2xl` | 24px | 72 | 36 | 60 | Large body |
| `text-3xl` | 30px | 90 | 45 | 75 | Subtitle |
| `text-4xl` | 36px | 108 | 54 | 90 | Title |
| `text-5xl` | 48px | 144 | 72 | 120 | Big title |
| `text-6xl` | 60px | 180 | 90 | 150 | Hero headline |
| `text-7xl` | 72px | 216 | 108 | 180 | Impact counter |
| `text-8xl` | 96px | 288 | 144 | 240 | Full-screen number |
| `text-9xl` | 128px | 384 | 192 | 320 | Giant display |

## Tailwind 4 spacing scale → rustmotion pixels

Reference: Tailwind CSS default spacing (4px base unit).

| Tailwind | CSS size | Mobile (×3) | Desktop (×1.5) | Usage |
|---|---|---|---|---|
| `4` | 16px | 48 | 24 | Small gap |
| `6` | 24px | 72 | 36 | Medium gap |
| `8` | 32px | 96 | 48 | Card padding |
| `10` | 40px | 120 | 60 | Large gap |
| `12` | 48px | 144 | 72 | Section gap |
| `16` | 64px | 192 | 96 | Scene layout gap |

## Complete sizing reference per device

### Mobile Portrait (1080×1920)

| Element | Size | Tailwind equivalent |
|---|---|---|
| **Title text** | font-size: 108–144 | `text-4xl` to `text-5xl` |
| **Subtitle text** | font-size: 72–90 | `text-2xl` to `text-3xl` |
| **Body text** | font-size: 54–72 | `text-lg` to `text-2xl` |
| **Label / small text** | font-size: 42–48 | `text-sm` to `text-base` |
| **Icon (hero/standalone)** | 160–200px | Large and impactful |
| **Icon (in card)** | 72–96px | Clear and recognizable |
| **Icon (inline/small)** | 48–60px | Chevrons, decorative |
| **Card width** | 960–1020px | ~90% of viewport |
| **Card padding** | 40–48px | Tailwind `p-10` to `p-12` |
| **Card border-radius** | 28–36px | Generously rounded |
| **Card row** | max 3 cols | More = unreadable |
| **Counter** | font-size: 180–288 | `text-6xl` to `text-8xl` |
| **Badge** | badge_size: "lg" + **font-size: 36–42** | Built-in "lg" = only 18px font (tiny on mobile!). MUST override with `style.font-size` |
| **Terminal font-size** | 28–32px | Readable monospace |
| **Terminal width** | 980–1020px | Near full width |
| **Chart size** | width: 940, height: 400+ | Fill the space |
| **Timeline width** | 940–980px | Near full width |
| **Timeline node_radius** | 36–44px | Visible nodes |
| **Timeline font_size** | 32–36px | Readable labels |
| **Scene layout gap** | 48–72px | Tailwind `gap-12` to `gap-16` |
| **CTA button** | width: 720–900px, padding: 36 | Thumb-friendly |
| **max_width (text)** | 960–1000px | Almost full width |
| **Glow radius** | 32–44px | Visible halo |
| **Particle (halo) size_range** | {min: 60, max: 140} | Visible blobs |

### Desktop Landscape (1920×1080)

| Element | Size | Tailwind equivalent |
|---|---|---|
| **Title text** | font-size: 54–72 | `text-4xl` to `text-5xl` |
| **Subtitle text** | font-size: 36–45 | `text-2xl` to `text-3xl` |
| **Body text** | font-size: 24–30 | `text-base` to `text-lg` |
| **Label / small text** | font-size: 21–24 | `text-sm` to `text-base` |
| **Icon (hero)** | 72–96px | |
| **Icon (in card)** | 40–56px | |
| **Card width** | 800–1400px | 40-70% of viewport |
| **Card padding** | 24–36px | |
| **Counter** | font-size: 90–144 | |
| **Badge** | badge_size: "md" | Default font-size OK for desktop |
| **Terminal font-size** | 18–22px | |
| **Scene layout gap** | 24–36px | |
| **max_width (text)** | 800–1200px | |

### Square (1080×1080)

| Element | Size | Tailwind equivalent |
|---|---|---|
| **Title text** | font-size: 90–120 | `text-4xl` to `text-5xl` |
| **Subtitle text** | font-size: 60–75 | `text-2xl` to `text-3xl` |
| **Body text** | font-size: 40–50 | `text-base` to `text-lg` |
| **Icon (hero)** | 120–150px | |
| **Card width** | 920–1020px | ~90% of viewport |
| **Counter** | font-size: 120–180 | |
| **Badge** | badge_size: "lg" + **font-size: 32–36** | Override font-size for readability |
| **Scene layout gap** | 36–48px | |

## Key principle

**Think in Tailwind classes first, then scale.** If your title should be `text-4xl` (36px CSS), multiply by 3 for mobile = 108px in rustmotion. This ensures consistent, readable results across devices.

## Layout adjustments per device

- **Mobile**: Prefer vertical stacking (column). Max 2–3 items per row. Cards should be nearly full-width (90%+). Use larger gaps. Break horizontal flows into stacked layouts. **CRITICAL: Add scene-level `padding` (48–60px) to prevent elements from touching screen edges.** All child widths (cards, timelines, terminals) must account for this padding: max child width = video width − 2 × scene padding. For 1080px with 48px padding → max child width = 984px.
- **Desktop**: Can use horizontal rows with 3–5 items. Cards at 50–70% width. Tighter gaps. Scene padding optional (24px).
- **Square**: Hybrid — 2–3 items per row, 85–95% width cards. Scene padding 36–48px.

## Viewport overflow safety

Sizing rules above are guidelines — `rustmotion validate` is the source of truth. It refuses any scenario whose layout tree leaves the device viewport. See [geometry-safety.md](geometry-safety.md):

- `text` wraps automatically at the parent's max width — leave `style.wrap` at its default (`true`) unless you have a finite `max-width`.
- `codeblock` / `terminal` auto-scroll when content exceeds their `size` — leave `auto_scroll: true` (default).
- Long single-line content that should bleed must use `marquee`, never `text` with `wrap: false`.

## BAD: Using desktop sizes on mobile

```json
{
  "type": "text",
  "content": "Title",
  "style": { "font-size": 48, "color": "#FFFFFF" }
}
```
48px ÷ 3 = 16px CSS → `text-base` → body text, NOT a title!

## GOOD: Mobile-scaled title

```json
{
  "type": "text",
  "content": "Title",
  "style": { "font-size": 108, "color": "#FFFFFF" }
}
```
108px ÷ 3 = 36px CSS → `text-4xl` → proper title size!

## BAD: 4-column card layout on mobile

```json
{
  "type": "card",
  "size": { "width": 920, "height": "auto" },
  "style": { "flex-direction": "row" },
  "children": [
    { "size": { "width": 180 } },
    { "size": { "width": 180 } },
    { "size": { "width": 180 } },
    { "size": { "width": 180 } }
  ]
}
```
4 × 180px = 60px CSS each → cramped, unreadable.

## GOOD: Stacked or 2-col on mobile

```json
{
  "type": "card",
  "size": { "width": 1000, "height": "auto" },
  "style": { "flex-direction": "column", "gap": 24 },
  "children": [
    {
      "type": "card",
      "size": { "width": 1000, "height": "auto" },
      "style": { "flex-direction": "row", "align-items": "center", "gap": 24 },
      "children": [
        { "type": "icon", "size": { "width": 72, "height": 72 } },
        { "type": "text", "style": { "font-size": 48 } }
      ]
    }
  ]
}
```
