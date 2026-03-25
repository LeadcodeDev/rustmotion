# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.
See .claude/skills/ for detailed instructions on generating rustmotion scenarios.

## Règle obligatoire

Tout JSON de scénario généré doit être validé avec `rustmotion validate` avant d'être présenté à l'utilisateur.

## Encodage

- ffmpeg est auto-détecté et utilisé par défaut (10-bit H.264, meilleure qualité sur les gradients sombres)
- Sans ffmpeg, le fallback openh264 intégré encode en 8-bit
- Pour les vidéos avec des gradients sombres, recommander `--codec prores` pour une qualité maximale

## Composants disponibles (51)

### Basiques
`text`, `shape`, `image`, `icon`, `svg`, `video`, `gif`, `caption`, `rich_text`, `gradient_text`

### Conteneurs
`card`, `flex`, `grid`, `container`, `positioned`

### Data Visualization
- `chart` — 12 types: bar, line, pie, donut, horizontal_bar, area, stacked_bar, radar, scatter, radial_bar, funnel, waterfall. Supporte axes/grilles/labels.
- `gauge` — jauge semi-circulaire pour KPIs
- `sparkline` — mini-chart inline sans axes
- `stat` — carte KPI composite (valeur + label + tendance + sparkline)
- `heatmap` — grille colorée type GitHub contributions
- `treemap` — rectangles proportionnels (slice-and-dice)
- `dot_map` — carte mondiale en dot-pattern avec points de données, pulse, lat/lng
- `progress` — barre linéaire ou circulaire
- `counter` — compteur animé (standalone uniquement, pas dans les cards)
- `table` — tableau avec column_widths, column_align, cell_padding, show_borders

### UI Components
- `badge` — pill avec icon, dot indicator, pulse animation, count badge
- `avatar` / `avatar_group` — avatar circulaire / groupe empilé avec "+N"
- `switch` — toggle animé on/off avec toggle_at
- `slider` — curseur horizontal animé avec animate_to/animate_at
- `rating` — étoiles avec remplissage partiel animé
- `kbd` — touche clavier visuelle (effet 3D)
- `tooltip` — label flottant avec flèche directionnelle
- `notification` — toast fade-in/out avec stack push (info/success/warning/error)
- `pill_nav` — tabs avec pill indicator animé entre onglets
- `list` — liste bullet/numbered/checklist avec icônes
- `stepper` — étapes numérotées connectées avec progression animée
- `comparison` — vue avant/après avec divider animé
- `countdown` — timer digital flip-clock style
- `marquee` — texte défilant continu
- `skeleton` — placeholder de chargement avec shimmer (rectangle/circle/text)
- `tag_cloud` — nuage de mots avec tailles pondérées
- `callout` — bulle avec flèche
- `divider` — séparateur visuel

### Code & Terminal
- `codeblock` — code syntax-highlighted avec reveal, diff mode (`diff: true`), state transitions
- `terminal` — terminal avec chrome macOS, reveal typewriter + curseur clignotant

### Diagrammes
`arrow`, `connector`, `timeline`, `line`

### Média
`mockup`, `lottie`, `cursor`, `particle`, `qrcode`
