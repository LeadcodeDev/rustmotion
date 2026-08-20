# Rule: Icon Sizing Hierarchy & Card Spacing

Icons that are too small disappear on screen. Icons without a consistent sizing framework create visual incoherence across scenes. This rule defines three icon roles, their sizes per device, and the minimum spacing rules for card layouts.

## Three icon roles

| Role | Description | Mobile 1080×1920 | Desktop 1920×1080 | Square 1080×1080 |
|---|---|---|---|---|
| **Hero / focal** | Centerpiece of a scene; one per scene max | 160–200px | 80–100px | 120–160px |
| **Card / feature** | Represents a feature or category inside a card | 72–96px | 40–56px | 60–80px |
| **Inline / decorative** | Next to text, chevrons, small indicators | 48–60px | 24–32px | 40–48px |

## Icon-to-text size relationship

When an icon appears adjacent to text (row layout), the icon height should be approximately **1.5× the adjacent text's font-size**:

| Adjacent text font-size | Icon size |
|---|---|
| Body 48px (mobile) | Icon 72–80px |
| Subtitle 72px (mobile) | Icon 96–108px |
| Label 42px (mobile) | Icon 54–64px |
| Body 24px (desktop) | Icon 36–40px |
| Subtitle 36px (desktop) | Icon 48–56px |

If the icon is the focal point and the text is secondary (caption below), use the hero role size regardless.

## Card spacing minimums

These are **hard minimums**. Below these values, cards visually merge and content becomes cramped.

### Between cards (gap)

| Device | Minimum gap between cards |
|---|---|
| Mobile | 24px |
| Desktop | 16px |
| Square | 20px |

### Internal card padding

| Device | Minimum internal padding |
|---|---|
| Mobile | 40–48px (Tailwind `p-10` to `p-12` equivalent) |
| Desktop | 24–32px |
| Square | 32–40px |

### Card row layout per device

| Number of cards | Mobile | Desktop | Square |
|---|---|---|---|
| 2 | Row layout | Row layout | Row layout |
| 3 | Column layout (stacked) | Row layout | Row or 2+1 |
| 4 | 2×2 grid | Row or 2×2 grid | 2×2 grid |
| 5+ | Multiple scenes or scroll | Row up to 4, then wrap | Multiple scenes |

**Critical:** On mobile, never put more than 2 items in a single row. Each item would be less than 500px wide (< 167px CSS) — too narrow for readable content.

## BAD: Hero icon too small on mobile

```json
{
  "type": "icon",
  "icon": "lucide:rocket",
  "style": { "width": 48, "height": 48, "color": "#6366F1" }
}
```
48px ÷ 3 = 16px CSS → postage stamp. Invisible as a hero element.

## GOOD: Hero icon at correct size on mobile

```json
{
  "type": "icon",
  "icon": "lucide:rocket",
  "style": {
    "width": 180,
    "height": 180,
    "color": "#6366F1",
    "animation": [{ "name": "scale_in", "duration": 0.6 }]
  }
}
```
180px ÷ 3 = 60px CSS → prominent, visible, impactful. ✓

## BAD: 4 cards in a row on mobile

```json
{
  "type": "card",
  "style": { "flex-direction": "row", "gap": 16 },
  "children": [
    { "type": "card", "style": { "width": 230 }, ... },
    { "type": "card", "style": { "width": 230 }, ... },
    { "type": "card", "style": { "width": 230 }, ... },
    { "type": "card", "style": { "width": 230 }, ... }
  ]
}
```
Each card is 230px = 77px CSS → icon and text completely unreadable.

## GOOD: 2×2 grid on mobile

`grid-template-columns` takes an array (`["1fr", "1fr"]`), never a raw CSS shorthand string like `"1fr 1fr"` — a bare string fails to deserialize (`Vec<GridTrack>` expected) and drops the card. Grid containers also need an explicit `height` (not `"auto"`) — see [rules/grid-card-height.md](grid-card-height.md).

```json
{
  "type": "card",
  "style": {
    "width": 984,
    "height": 640,
    "display": "grid",
    "grid-template-columns": ["1fr", "1fr"],
    "grid-template-rows": ["1fr", "1fr"],
    "gap": 24
  },
  "children": [
    { "type": "card", "style": { "padding": 40 }, "children": [] },
    { "type": "card", "style": { "padding": 40 }, "children": [] },
    { "type": "card", "style": { "padding": 40 }, "children": [] },
    { "type": "card", "style": { "padding": 40 }, "children": [] }
  ]
}
```
Each cell fills its grid track (~480px wide here) = 160px CSS → readable content. 24px gap. ✓

## BAD: Icon inconsistency across scenes

```
Scene 1: icon 180px (hero role)
Scene 2: icon 48px  (same role, wrong size — 4× smaller)
Scene 3: icon 120px (hero role)
```
No visual language — each scene feels like a different product.

## GOOD: Consistent icon role across scenes

```
Scene 1: hero icon 180px  → role: hero
Scene 2: feature icons 80px each in cards → role: card
Scene 3: hero icon 180px  → role: hero (same as scene 1)
```
Defined roles, predictable visual weight. ✓
