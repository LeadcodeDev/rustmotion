# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.
See .claude/skills/ for detailed instructions on generating rustmotion scenarios.

## Règle obligatoire

Tout JSON de scénario généré doit être validé avec `rustmotion validate` avant d'être présenté à l'utilisateur.

## Encodage

- ffmpeg est auto-détecté et utilisé par défaut (10-bit H.264, meilleure qualité sur les gradients sombres)
- Sans ffmpeg, le fallback openh264 intégré encode en 8-bit
- Pour les vidéos avec des gradients sombres, recommander `--codec prores` pour une qualité maximale

## Composants disponibles (37)

### Basiques
`text`, `shape`, `image`, `icon`, `svg`, `video`, `gif`, `caption`, `rich_text`

### Conteneurs
`card`, `flex`, `grid`, `container`, `positioned`

### Data Visualization
- `chart` — 12 types: bar, line, pie, donut, horizontal_bar, area, stacked_bar, radar, scatter, radial_bar, funnel, waterfall. Supporte axes/grilles/labels.
- `gauge` — jauge semi-circulaire pour KPIs
- `sparkline` — mini-chart inline sans axes
- `stat` — carte KPI composite (valeur + label + tendance + sparkline)
- `progress` — barre linéaire ou circulaire
- `counter` — compteur animé (standalone uniquement, pas dans les cards)
- `table` — tableau avec column_widths, column_align, cell_padding, show_borders

### UI Components
- `badge` — pill avec icon, dot indicator, pulse animation, count badge
- `avatar` / `avatar_group` — avatar circulaire / groupe empilé avec "+N"
- `kbd` — touche clavier visuelle (effet 3D)
- `tooltip` — label flottant avec flèche directionnelle
- `marquee` — texte défilant continu
- `skeleton` — placeholder de chargement avec shimmer (rectangle/circle/text)
- `callout` — bulle avec flèche
- `divider` — séparateur visuel

### Code & Terminal
- `codeblock` — code syntax-highlighted avec reveal, diff mode (`diff: true`), state transitions
- `terminal` — terminal avec chrome macOS, reveal typewriter + curseur clignotant

### Diagrammes
`arrow`, `connector`, `timeline`, `line`

### Média
`mockup`, `lottie`, `cursor`, `particle`, `qrcode`
