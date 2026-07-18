# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.
See .claude/skills/ for detailed instructions on generating rustmotion scenarios.

## Règle obligatoire

Tout JSON de scénario généré doit être validé avec `rustmotion validate` avant d'être présenté à l'utilisateur. Le validateur fait deux passes : **schema** et **geometry** (détection de débordement viewport). Les deux doivent passer.

## Sécurité géométrique (viewport)

Aucun contenu textuel ne doit dépasser du device. Trois propriétés contrôlent ce comportement :

- `style.wrap` (default `true`) sur `text` : laisse-le à `true` pour wrapper sur la largeur du parent. `wrap: false` est légitime uniquement si un `max-width` finit + `font-size` raisonnable garantissent que la ligne tient. Le validateur émet `unwrappable_text_overflow` sinon.
- `auto_scroll` (default `true`) sur `codeblock` et `terminal` : quand le contenu dépasse la hauteur du `size`, le moteur scrolle (clip + translate) sans réduire la `font-size`. `auto_scroll: false` → `auto_scroll_disabled_overflow`.
- `style.overflow` (default `visible`) sur les conteneurs : sémantique CSS. `hidden` clippe au bord du parent. Le validateur ne se plaint que si le contenu sort du **viewport**, pas d'un parent `visible`.

`marquee` et `cursor` sont exemptés (leur rôle est de bleed).

CLI :
- `rustmotion validate -f file.json` — schema + geometry
- `--fix` — auto-fix sûr (`wrap: true`, `auto_scroll: true`)
- `--report r.json` — rapport JSON
- `--strict-anim` — vérification frame par frame
- `--strict-attrs` — promeut en erreurs les attributs inconnus (détection schéma + did-you-mean, activée par défaut en warnings)
- `--lenient` — warnings au lieu d'errors

## Encodage

- ffmpeg est auto-détecté et utilisé par défaut (10-bit H.264, meilleure qualité sur les gradients sombres)
- Sans ffmpeg, le fallback openh264 intégré encode en 8-bit
- Pour les vidéos avec des gradients sombres, recommander `--codec prores` pour une qualité maximale

## Composants disponibles (51)

### Basiques
`text`, `shape`, `image`, `icon`, `svg`, `video`, `gif`, `caption`, `rich_text`, `gradient_text`

### Conteneurs
`card`, `flex`, `grid`, `div` (alias de `container`), `container`, `positioned`

> `div` = layout pur sans décoration visuelle (HTML `<div>`). `card` = même chose mais avec fond/border-radius/ombre attendus.

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

### Render Pipeline (CSS engine)

Le moteur utilise un pipeline **box_tree → layout_pass → paint_pass** inspiré des navigateurs web :

1. **box_tree** (`box_builder.rs`) — construit un arbre de `BoxNode { css: CssStyle, children, intrinsic }` depuis les composants JSON résolus
2. **layout_pass** (`engine/layout_pass.rs`) — orchestre taffy pour calculer les `BoxLayout { x, y, width, height }` de chaque nœud. Les feuilles avec un `IntrinsicMeasure` (texte, image, codeblock) sont mesurées via une `measure_fn`.
3. **paint_pass** (`engine/paint_pass.rs`) — descend l'arbre, applique transform/opacity, peint les décorations (background, border, shadow), délègue au `Painter` du composant pour le contenu.

Chaque composant implémente le trait `Painter` :

```rust
pub trait Painter {
    fn paint_content(&self, canvas: &Canvas, layout: &BoxLayout, props: &AnimatedProperties, ctx: &PaintCtx);
    fn intrinsic_size(&self, available: AvailableSize, ctx: &MeasureCtx) -> Option<(f32, f32)> { None }
}
```

`PaintCtx` contient : `time`, `scene_duration`, `fps`, `frame_index`, `video_width`, `video_height`, `stagger_offset`.

### Structure des crates

```
crates/
├── rustmotion-core/src/
│   ├── css/                    # Modèle CSS
│   │   ├── style.rs            # CssStyle (propriétés CSS kebab-case)
│   │   ├── units.rs            # Length, LengthPercentage (px, %, em, rem, vw, vh)
│   │   ├── cascade.rs          # Héritage color/font-* parent → enfant
│   │   ├── taffy_bridge.rs     # CssStyle → taffy::Style
│   │   └── animation.rs        # Résolution des animations → override CssStyle
│   ├── engine/
│   │   ├── box_tree.rs         # BoxNode, BoxKind, IntrinsicMeasure
│   │   ├── layout_pass.rs      # Orchestration taffy, BoxLayout résultant
│   │   ├── paint_pass.rs       # Walk top-down, décorations, dispatch Painter
│   │   ├── animator.rs         # Résolution animations, easing, spring solver
│   │   ├── transition.rs       # Transitions entre scènes
│   │   ├── renderer/           # Primitives Skia (colors, fonts, shapes, text)
│   │   └── text/cosmic.rs      # Bridge cosmic-text ↔ Skia (mesure + glyphs)
│   ├── schema/                 # Modèles de données JSON
│   │   ├── scenario.rs         # Scenario, ResolvedScenario, View, Scene, VideoConfig
│   │   ├── style.rs            # Specialized types (CardBorder, CardShadow, Fill, etc.)
│   │   ├── background.rs       # AnimatedBackground, BackgroundPreset
│   │   ├── animation.rs        # EasingType, AnimationPreset, PresetConfig
│   │   ├── codeblock_types.rs  # CodeblockChrome, CodeblockState
│   │   └── video.rs            # AnimationEffect, Size, ShapeType, Stroke
│   └── traits/
│       ├── painter.rs          # Painter trait + PaintCtx + AvailableSize + MeasureCtx
│       ├── animatable.rs       # Animatable trait
│       ├── timed.rs            # Timed trait + TimingConfig
│       └── styled.rs           # Styled trait
│
├── rustmotion-components/src/
│   ├── lib.rs                  # Enum Component + dispatch (as_painter, as_animatable, etc.)
│   ├── box_builder.rs          # build_scene() → BuiltScene (components + stagger_delays)
│   ├── intrinsic.rs            # TextIntrinsic, BadgeIntrinsic, CounterIntrinsic, etc.
│   ├── legacy_dispatch.rs      # LegacyPaintDispatcher (bridge NodeId → Painter)
│   ├── chart/                  # 10 fichiers (mod + bar/line/pie/radar/scatter/radial/funnel/waterfall/axes)
│   └── *.rs                    # Un fichier par composant (impl Painter)
│
└── rustmotion-cli/src/
    └── commands/               # validate, render, schema, info
```

### Ajouter un nouveau composant

1. Créer `crates/rustmotion-components/src/mon_composant.rs` avec struct serde + `impl Painter` (`paint_content`)
2. Ajouter `rustmotion_core::impl_traits!(MonComposant { Animatable => animation, Timed => timing, Styled => style });`
3. Ajouter le variant dans l'enum `Component` dans `lib.rs`
4. Ajouter les match arms dans les méthodes de dispatch (`as_painter`, `as_animatable`, `as_timed`, `as_styled`)
5. Ajouter `pub mod mon_composant;` et `pub use mon_composant::MonComposant;` dans `lib.rs`
6. Si le composant a une taille fixe: la déclarer via apply_intrinsic_overrides dans box_builder.rs
7. Si le composant mesure son propre contenu : ajouter `XxxIntrinsic` dans `intrinsic.rs`

### Tests

```bash
cargo test --workspace        # ~200 tests (layout + serde round-trip + pixel regressions + smoke)
cargo check                   # Vérification compilation
rustmotion validate file.json # Validation scénario
```
