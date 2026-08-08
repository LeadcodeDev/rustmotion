# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.
See .claude/skills/ for detailed instructions on generating rustmotion scenarios.

## Règle obligatoire

Tout JSON de scénario généré doit être validé avec `rustmotion validate` avant d'être présenté à l'utilisateur. Le validateur fait deux passes : **schema** et **geometry** (détection de débordement viewport). Les deux doivent passer.

## Sécurité géométrique (viewport)

Aucun contenu textuel ne doit dépasser du device. Trois propriétés contrôlent ce comportement :

- `style.white-space` (default `normal`, donc wrap actif) sur `text` : le texte wrap sur la largeur du parent par défaut. `white-space: "nowrap"` (ou `"pre"`) est légitime uniquement si un `max-width` fini + `font-size` raisonnable garantissent que la ligne tient. Le validateur émet `unwrappable_text_overflow` sinon. Il n'existe pas de champ `style.wrap` — c'est un vocabulaire hérité de l'ancien modèle de style, supprimé de `CssStyle`. Voir [rules/geometry-safety.md](.claude/skills/rustmotion/rules/geometry-safety.md).
- `auto_scroll` (default `true`) sur `codeblock` et `terminal` : quand le contenu dépasse la hauteur du `size`, le moteur scrolle (clip + translate) sans réduire la `font-size`. `auto_scroll: false` → `auto_scroll_disabled_overflow`.
- `style.overflow` (default `visible`) sur les conteneurs : sémantique CSS. `hidden` clippe au bord du parent. Le validateur ne se plaint que si le contenu sort du **viewport**, pas d'un parent `visible`.

`marquee` et `cursor` sont exemptés (leur rôle est de bleed).

CLI :
- `rustmotion validate -f file.json` — schema + geometry
- `--fix` — auto-fix sûr : `auto_scroll: true` sur `auto_scroll_disabled_overflow`, et retrait de `style.white-space` sur `unwrappable_text_overflow` (retour au défaut `normal`, donc au wrapping). Les débordements de viewport et de boîte ne sont jamais corrigés automatiquement : ils demandent un arbitrage de mise en page.
- `--report r.json` — rapport JSON
- `--strict-anim` — vérification frame par frame ; ajoute la détection `animated_text_overflow` (transform animé qui sort du viewport à un instant échantillonné)
- `--strict-attrs` — promeut en erreurs les attributs inconnus (détection schéma + did-you-mean, activée par défaut en warnings)
- `--lenient` — warnings au lieu d'errors

## Encodage

- ffmpeg est auto-détecté et utilisé par défaut (10-bit H.264, meilleure qualité sur les gradients sombres)
- Sans ffmpeg, le fallback openh264 intégré encode en 8-bit
- Pour les vidéos avec des gradients sombres, recommander `--codec prores` pour une qualité maximale

## Composition : `scenes` vs `composition` (vues `slide` / `world`)

Un scénario est soit une liste plate `scenes` (racine) — implicitement enveloppée dans une seule vue `slide` — soit un `composition: [...]` explicite, un tableau de **vues** typées `"slide"` ou `"world"`. Les deux sont mutuellement exclusifs (`CompositionAndScenesConflict` si les deux sont présents).

Dans une vue `slide`, les `transition` entre scènes sont des **composites pixel de deux frame-buffers déjà rendus** (fade, wipe, zoom, flip, iris, slide…) : aucun élément ne survit à la coupe, seuls les pixels sont mélangés.

La vue **`world`** est le seul mécanisme qui produit une continuité réelle entre beats : une caméra virtuelle se déplace en continu à travers un espace 2D où chaque scène occupe une position (`world-position`), avec un fondu de recouvrement pendant le pan au lieu d'une coupe. C'est la brique à utiliser pour une vidéo qui doit se lire comme un plan continu, sans limite de scène perceptible. Voir [rules/world-view.md](.claude/skills/rustmotion/rules/world-view.md) pour le modèle de coordonnées (le piège `world-position` = waypoint caméra, pas origine de scène), la recette du halo ambiant en `view.background`, et un exemple multi-beat validé.

**Piège de casing à connaître :** `world-position` (scène) est en kebab-case, alors que son voisin `freeze_at` (même struct `Scene`) est en snake_case. Vraie inconsistance du schéma, pas une faute de frappe — copier la casse telle quelle.

## Composants disponibles (57)

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
`mockup`, `lottie`, `cursor`, `particle`, `qr_code`

### Audio
- `waveform` — visualisation d'onde audio réactive au volume de la piste
- `audio_spectrum` — barres de spectre audio réactives (FFT)

> Voir [rules/audio-reactive.md](.claude/skills/rustmotion/rules/audio-reactive.md) pour lier un composant à une piste `audio` via `style.audio-reactive`.

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
│   │   └── text/cosmic.rs      # Bridge cosmic-text — PAS branché sur le rendu réel
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
