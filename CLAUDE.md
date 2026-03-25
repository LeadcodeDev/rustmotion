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

## Architecture

### Render Pipeline (Flutter-like)

Chaque composant suit un cycle **measure → layout → paint** inspiré de Flutter :

1. `widget.measure(constraints)` → `(width, height)` — calcule la taille souhaitée
2. `widget.layout(constraints)` → `LayoutNode` — calcule l'arbre de layout (récursif pour les conteneurs)
3. `widget.paint(canvas, ctx)` → dessine sur le canvas Skia

La méthode `paint()` reçoit un `PaintContext` riche contenant :
- **Timing** : `ctx.time`, `ctx.scene_duration`, `ctx.fps`, `ctx.frame_index`
- **Layout** : `ctx.layout` (LayoutNode complet), `ctx.width()`, `ctx.height()`
- **Parent info** : `ctx.parent_size`, `ctx.absolute_position`, `ctx.depth`
- **Animation** : `ctx.props` (AnimatedProperties résolues), `ctx.stagger_offset`
- **Vidéo** : `ctx.video_width`, `ctx.video_height`

Pour les conteneurs qui appellent `render_children`, utiliser `ctx.as_render_context()` pour convertir vers le pipeline de rendu.

### Structure des modules

```
src/
├── components/           # 51 composants (chacun implémente Widget)
│   ├── mod.rs            # Enum Component + dispatch (as_widget, as_animatable, etc.)
│   ├── chart/            # 10 fichiers (mod + bar/line/pie/radar/scatter/radial/funnel/waterfall/axes)
│   └── *.rs              # Un fichier par composant
├── engine/
│   ├── render/           # Pipeline de rendu (5 fichiers)
│   │   ├── mod.rs        # render_component, render_children, render_overlays
│   │   ├── scene.rs      # render_frame_v2, render_scene_frame, compute_root_layout
│   │   ├── background.rs # draw_animated_background, gradients, halo, grid dots
│   │   └── transforms.rs # draw_3d_shadow, draw_inner_shadow
│   ├── codeblock/        # Rendu codeblock (6 fichiers)
│   │   ├── mod.rs        # render_codeblock, render_codeblock_v2
│   │   ├── highlight.rs  # Syntect, thèmes, highlight_code
│   │   ├── chrome.rs     # Barre titre macOS
│   │   ├── reveal.rs     # Typewriter, line-by-line
│   │   ├── diff.rs       # State transitions, word diff
│   │   └── dimensions.rs # compute_code_dimensions
│   ├── animator.rs       # Résolution animations, easing, spring solver
│   └── renderer.rs       # Primitives de dessin Skia
├── schema/               # Modèles de données (5 fichiers)
│   ├── scenario.rs       # Scenario, View, Scene, VideoConfig
│   ├── style.rs          # LayerStyle, Spacing, FontWeight, CardDirection
│   ├── background.rs     # AnimatedBackground, BackgroundPreset
│   ├── animation.rs      # EasingType, AnimationPreset, PresetConfig
│   ├── codeblock_types.rs # CodeblockChrome, CodeblockState
│   └── video.rs          # AnimationEffect, Size, ShapeType, Fill
├── layout/               # Moteurs de layout flex/grid
│   ├── flex.rs           # CSS flexbox
│   ├── grid.rs           # CSS grid
│   ├── tree.rs           # LayoutNode
│   └── constraints.rs    # Constraints (Flutter-style)
├── traits/               # Traits des composants
│   ├── widget.rs         # Widget trait (paint/measure/layout) + PaintContext + RenderContext
│   ├── styled.rs         # Styled + StyledExt (builder pattern)
│   ├── container.rs      # Container, FlexContainer, GridContainer
│   ├── animatable.rs     # Animatable trait
│   └── timed.rs          # Timed trait + TimingConfig
└── macros.rs             # impl_traits! macro
```

### Ajouter un nouveau composant

1. Créer `src/components/mon_composant.rs` avec struct + `impl Widget` (`paint`, `measure`)
2. Ajouter `crate::impl_traits!(MonComposant { Animatable => style, Timed => timing, Styled => style });`
3. Ajouter le variant dans l'enum `Component` dans `src/components/mod.rs`
4. Ajouter les match arms dans les 5 méthodes de dispatch (`as_widget`, `as_animatable`, `as_timed`, `as_styled`, `as_container`)
5. Ajouter `pub mod mon_composant;` et `pub use mon_composant::MonComposant;` dans `mod.rs`

### Tests

```bash
cargo test                    # 31 tests (layout + variables + smoke)
cargo check                   # Vérification compilation
rustmotion validate file.json # Validation scénario
```
