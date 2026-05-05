# Rule: Scene Pacing & Cognitive Load

A scene that moves too fast leaves the viewer confused. A scene that's too slow feels sluggish. This rule provides concrete formulas to calculate scene durations based on content and animation budgets.

## Duration formula

```
scene_duration ≥ max(animation_budget + dwell, reading_time)
```

Where:
- **`animation_budget`** = time at which the last element finishes its entrance animation (see [animation-completion-budget.md](animation-completion-budget.md))
- **`dwell`** = 0.5s for simple scenes (1-2 elements), 1.0s for data-heavy or text-heavy scenes
- **`reading_time`** = `word_count ÷ 2.5` (viewer reads ~150 words/min = 2.5 words/sec on screen)

**Always take the maximum** — reading time governs on text-heavy scenes; animation budget governs on visual-only scenes.

## Scene duration reference table

| Content type | Suggested duration |
|---|---|
| Single icon + title hero | 2.5–3.5s |
| Title + subtitle | 3.0–4.0s |
| Title + body text (30–50 words) | 5.0–7.0s |
| 3× feature cards with text | 5.0–6.0s |
| Counter animation | 3.0–4.0s (`animation_budget` + 1s dwell) |
| Codeblock typewriter reveal | 6.0–12.0s (depends on line count) |
| Data chart with labels | 5.0–7.0s |
| Dashboard with multiple stats | 6.0–8.0s |
| CTA / outro | 2.5–3.5s |
| Transition / breathing scene | 1.5–2.5s |

## Worked examples

**Scene: title (3 words) + subtitle (6 words)**
- Word count: 9 → reading time: `9 ÷ 2.5 = 3.6s`
- Animation budget: title at `delay: 0`, duration `0.6s` → finishes `0.6s`; subtitle at `delay: 0.3`, duration `0.6s` → finishes `0.9s`
- Budget + dwell: `0.9 + 0.5 = 1.4s`
- Max(3.6, 1.4) = **3.6s** → set duration to **4.0s** (round up)

**Scene: 5 feature cards with icon + 2 lines each (≈ 10 words/card = 50 words total)**
- Reading time: `50 ÷ 2.5 = 20s` → **split into 2-3 separate scenes**
- Per scene (≈ 17 words, 2 cards): reading time `6.8s`; stagger budget `0 + 0.2 + 0.6 = 0.8s`
- Max(6.8, 1.8) = **6.8s** → set duration to **7.0s**

**Rule of thumb:** If reading time > 8s, the scene has too much content → split it.

## Animation density limit

Too many simultaneous entrances create visual chaos and overwhelm the viewer.

| Device | Maximum animated components per scene |
|---|---|
| Mobile (1080×1920) | 5 |
| Desktop (1920×1080) | 7 |
| Square (1080×1080) | 6 |

"Animated component" = any component with an entrance animation. A counter counts as 1. A card with `stagger` counts as the number of children that animate.

If you exceed the limit: group related components into a `container` with a single shared animation, or split across scenes.

## Dense vs breathing scenes

**Dense scene** — 4+ components, stagger entrance, data or feature cards. Needs longer duration. Examples: features grid, comparison, dashboard.

**Breathing scene** — 1-2 components, big typography, minimal elements. Can be 2-3s. Examples: chapter title, transition, hero quote.

**Rule: Never place two dense scenes back-to-back.** Insert a breathing scene (2-3s, simple text or icon) between them to give the viewer cognitive rest.

## BAD: Under-timed text-heavy scene

```json
{
  "duration": 2.0,
  "children": [
    { "type": "text", "content": "Automate your entire workflow in minutes. No code required. Works with 200+ integrations out of the box.", "style": { "font-size": 54 } }
  ]
}
```
21 words → reading time = `21 ÷ 2.5 = 8.4s`. Scene is 2.0s → text flashes and disappears before it can be read.

## GOOD: Duration matches reading time

```json
{
  "duration": 9.0,
  "children": [
    { "type": "text", "content": "Automate your entire workflow in minutes. No code required. Works with 200+ integrations out of the box.", "animation": [{ "name": "fade_in_up", "delay": 0.0, "duration": 0.8 }], "style": { "font-size": 54 } }
  ]
}
```
Animation finishes at 0.8s. Dwell = 8.2s. Reading time = 8.4s. Max(9.0, 9.2) → duration of 9.5s would be tighter. ✓

Or better: split into 2-3 shorter sentences across multiple scenes for better pacing.

## BAD: Two dense scenes in a row

```
Scene 1 (5s): 6-card feature grid with stagger
Scene 2 (5s): 4-stat dashboard with animations
Scene 3 (5s): another 5-card grid
```
No breathing room → viewer exhausted by scene 3.

## GOOD: Alternate dense and breathing

```
Scene 1 (5s): 6-card feature grid
Scene 2 (2.5s): "Trusted by 10,000+ teams" — breathing scene, big counter
Scene 3 (5s): 4-stat dashboard
Scene 4 (2.5s): CTA card
```
