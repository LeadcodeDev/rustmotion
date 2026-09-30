# CLAUDE.md

This file provides guidance to Claude Code when working with this repository.
See .claude/skills/ for detailed instructions on generating rustmotion scenarios.

## Règle obligatoire

Tout JSON de scénario généré doit être validé avec `rustmotion validate` avant d'être présenté à l'utilisateur. Le validateur fait deux passes : **schema** et **geometry** (détection de débordement viewport). Les deux doivent passer.

## Sécurité géométrique (viewport)

Aucun contenu textuel ne doit dépasser du device. Trois propriétés contrôlent ce comportement :

- `style.white-space` (default `normal`, donc wrap actif) sur `text` : le texte wrap sur la largeur du parent par défaut. `white-space: "nowrap"` (ou `"pre"`) est légitime uniquement si un `max-width` fini + `font-size` raisonnable garantissent que la ligne tient. Le validateur émet `unwrappable_text_overflow` sinon. Il n'existe pas de champ `style.wrap` — c'est un vocabulaire hérité de l'ancien modèle de style, supprimé de `CssStyle`. Voir [rules/geometry-safety.md](.claude/skills/rustmotion/rules/geometry-safety.md).
- `style.text-autofit` (default absent) sur `text` et `gradient_text` : réduit la `font-size` jusqu'à ce que le contenu tienne dans sa boîte. À réserver au texte piloté par des données, dont on ne peut pas connaître la longueur à l'avance — pas pour compenser une mise en page qu'on peut simplement dimensionner. Le rétrécissement s'arrête à un plancher de lisibilité calibré ; si ça ne suffit pas, **la violation est toujours signalée**. Seuls ces deux composants l'implémentent : le déclarer ailleurs est inerte.
- `style.overflow` (default `visible`) sur les conteneurs : sémantique CSS. `hidden` clippe au bord du parent. Le validateur ne se plaint que si le contenu sort du **viewport**, pas d'un parent `visible`.

`marquee` et `cursor` sont exemptés (leur rôle est de bleed).

CLI :
- `rustmotion validate -f file.json` — schema + geometry
- `--fix` — auto-fix sûr : retrait de `style.white-space` sur `unwrappable_text_overflow` (retour au wrapping), et `text-autofit: true` sur `content_overflows_box` pour `text`/`gradient_text`. Les débordements de viewport restent non corrigés : ils demandent un arbitrage de mise en page. `--fix` **refuse** d'écrire sur un scénario templaté, utilisant `include`, ou utilisant `for-each`/`use` — les index de chemin ne correspondraient plus à la source.
- `--report r.json` — rapport JSON
- `--strict-anim` — vérification frame par frame ; ajoute la détection `animated_text_overflow` (transform animé qui sort du viewport à un instant échantillonné). L'échantillonnage s'arrête à `scene.freeze_at`, puisque rien n'est rendu au-delà.
- `--strict-attrs` — promeut en erreurs les attributs inconnus (détection schéma + did-you-mean, activée par défaut en warnings)
- `--lenient` — warnings au lieu d'errors

## Google Fonts: the network is denied by default

A scenario declaring `fonts: [{ "family": "Inter", "source": "google", "weights": [400, 700] }]`
downloads the face from `fonts.googleapis.com` into the cache
(`~/.cache/rustmotion/fonts`). The declared consumer of a scenario is a model
generating JSON, so the file is **untrusted input** — and it is the file that
chooses the target of the request. Validating someone else's scenario should not
be a network operation, and on a CI runner it turns a lint step into an outbound
fetch.

`--allow-remote-fonts` (global, on any subcommand) is the only way to consent to
it. Without it, a weight **missing from the cache** is refused by naming the
family, the missing weights, the URL that was not called, and the directory to
drop a TTF into by hand:

```
Google Fonts: 'Zilla Slab Highlight' (weights 700) is not in the font cache, and
fetching it would reach https://fonts.googleapis.com/css2?family=... — a scenario
chooses that target, so it is denied by default.
```

A face **already in the cache** asks for nothing: the refusal fires at the exact
moment a request would leave. An offline render that has its fonts keeps working
without the flag.

## Icons: the network is denied by default too

`icon` resolves `lucide:home` through `api.iconify.design`, and a scenario picks
that name, so the same argument as fonts applies: the file chooses the target of
the request. An icon **missing from the cache** (`~/.cache/rustmotion/icons`) is
refused by name, with the URL that was not called and the path to drop an SVG
into. An icon already cached asks for nothing, so an offline render that has its
icons keeps working.

Two ways in, and the first is the one to reach for:

```bash
rustmotion icons check -f scenario.json      # what is missing, fetches nothing
rustmotion icons prefetch -f scenario.json   # fill the cache once
rustmotion render -f scenario.json           # then render offline, no flag
```

`prefetch` needs no `--allow-remote-icons`: running it *is* the opt-in. The flag
exists for a one-shot render where filling the cache first is not worth the extra
command.

> The gate sits **after** the cache read and after the pre-#425 name migration, so
> a cache filled under the old `{slug}-{colour}-{w}x{h}.svg` naming still resolves
> offline. Denying the network must not break a render that already has what it
> needs.

## Encodage

- ffmpeg is auto-detected and used by default. H.264 output is **8-bit** (`yuv420p`, `high` profile), which QuickTime and Safari play; `--codec h264_10bit` trades that for `yuv420p10le`/`high10`, which is banding-free on dark gradients and refused by both players.
- `--hardware-acceleration` sonde `ffmpeg -encoders` et bascule sur VideoToolbox/NVENC/QSV/AMF si la machine en offre un. Indisponible → message explicite et repli logiciel, jamais de bascule silencieuse. Le CRF n'a pas de sens sur la plupart des encodeurs matériels : le passer avec l'accélération produit un avertissement.
- `--frames a-b` rend une plage de frames en segment autonome, avec **sa** tranche d'audio (les pistes ne repartent pas de zéro). `rustmotion concat seg1.mp4 seg2.mp4 -o out.mp4` les recolle via le concat demuxer de ffmpeg. C'est la brique d'un rendu distribué.
- Sans ffmpeg, le fallback openh264 intégré encode en 8-bit
- Pour les vidéos avec des gradients sombres, recommander `--codec prores` pour une qualité maximale

## Factorisation : `components`, `for-each`, `use`

Ne duplique pas un sous-arbre. Si dix cartes ne diffèrent que par leurs données, écris-en une et itère — c'est le mode d'échec le plus fréquent de la génération, chaque copie étant une occasion de diverger.

```json
"components": { "stat_card": { "params": { "label": { "type": "string" } }, "template": { … } } },
"children": [{
  "for-each": [ { "label": "Revenue" }, { "label": "Users" } ],
  "template": { "use": "stat_card", "props": { "label": "$label" } }
}]
```

Chaque élément du `for-each` lie ses champs directement (`$label`), plus `$index` et `$item`. `params` a la forme de `config` ; omettre `default` rend le paramètre requis. La clé d'overrides est **`props`**, pas `config` — ce nom-là est réservé et serait sauté par la substitution.

`components` est local au fichier qui le déclare. On peut itérer sur un tableau venu d'une variable ; on ne peut pas instancier un composant défini dans un fichier inclus. Toute erreur — cycle, tableau manquant, composant inconnu, paramètre absent — est nommée et située. Voir [rules/templates-and-iteration.md](.claude/skills/rustmotion/rules/templates-and-iteration.md).

> `--fix` refuse de réécrire un scénario qui utilise ces directives : les index de chemin ne correspondent plus à la source. `validate` fonctionne normalement, sur l'arbre expansé.

## Composition : `scenes` vs `composition` (vues `slide` / `world`)

Un scénario est soit une liste plate `scenes` (racine) — implicitement enveloppée dans une seule vue `slide` — soit un `composition: [...]` explicite, un tableau de **vues** typées `"slide"` ou `"world"`. Les deux sont mutuellement exclusifs (`CompositionAndScenesConflict` si les deux sont présents).

Dans une vue `slide`, les `transition` entre scènes sont des **composites pixel de deux frame-buffers déjà rendus** (fade, wipe, zoom, flip, iris, slide, chromatic_wipe…) : aucun élément ne survit à la coupe, seuls les pixels sont mélangés.

> `chromatic_wipe` is a fast slide whose reveal edge splits into red/cyan at its peak, then recomposes. `direction` (`left`/`right`/`up`/`down`) orients it, `aberration` sets how far the channels split (`0` = a plain slide, `2` = fully doubled). The flash is zero at both ends: a leftover fringe on the last frame would bleed into the next scene.

La vue **`world`** est le seul mécanisme qui produit une continuité réelle entre beats : une caméra virtuelle se déplace en continu à travers un espace 2D où chaque scène occupe une position (`world-position`), avec un fondu de recouvrement pendant le pan au lieu d'une coupe. C'est la brique à utiliser pour une vidéo qui doit se lire comme un plan continu, sans limite de scène perceptible. Voir [rules/world-view.md](.claude/skills/rustmotion/rules/world-view.md) pour le modèle de coordonnées (le piège `world-position` = waypoint caméra, pas origine de scène), la recette du halo ambiant en `view.background`, et un exemple multi-beat validé.

**Piège de casing à connaître :** `world-position` (scène) est en kebab-case, alors que son voisin `freeze_at` (même struct `Scene`) est en snake_case. Vraie inconsistance du schéma, pas une faute de frappe — copier la casse telle quelle.

## Animated backgrounds

`scene["animated-background"]` (kebab-case, like `world-position`) accepts a `preset` from `gradient_shift`, `grid_dots`, `grid_lines`, `concentric_circles`, `halo`, `pixel_grid`, `heropattern`, with its config under a key of the same name, plus the common `x`/`y`/`speed`/`direction` that drift the texture.

> `grid_dots` marks the intersections and reads as a texture; `grid_lines` is a grid of ruled **lines** and reads as a structure — the one to put behind a chart or a code panel. Config: `cell`, `weight`, `color`, plus `major_every`/`major_weight` for the graph-paper effect.

## Composants disponibles (61)

### Basiques
`text`, `shape`, `image`, `icon`, `svg`, `video`, `gif`, `caption`, `rich_text`, `gradient_text`

### Conteneurs
`div` — seul type de conteneur. `card`, `flex`, `grid`, `container`, `positioned` sont acceptés comme alias JSON du même composant (compat historique avec les six types qui existaient avant leur fusion) : aucune différence de comportement, de style par défaut ou de décoration entre ces orthographes. `style.display` (`flex` par défaut, ou `grid`) pilote le layout — pas le nom du tag ; fond, `border-radius`, ombre sont de simples propriétés `style` disponibles sur ce composant comme sur n'importe quel autre, pas un attribut réservé à l'une des anciennes variantes.

### Data Visualization
- `chart` — 12 types: bar, line, pie, donut, horizontal_bar, area, stacked_bar, radar, scatter, radial_bar, funnel, waterfall. Supporte axes/grilles/labels.
- `gauge` — jauge semi-circulaire pour KPIs
- `sparkline` — mini-chart inline sans axes
- `stat` — carte KPI composite (valeur + label + tendance + sparkline)
- `heatmap` — grille colorée type GitHub contributions
- `treemap` — rectangles proportionnels (slice-and-dice)
- `dot_map` — carte mondiale en dot-pattern avec points de données, pulse, lat/lng. `projection: "orthographic"` en fait un globe, avec occultation de l'hémisphère caché et arcs en grand cercle — voir [rules/dot-map-orthographic.md](.claude/skills/rustmotion/rules/dot-map-orthographic.md).
- `progress` — barre linéaire ou circulaire
- `counter` — compteur animé (standalone uniquement, pas dans les cards)
- `number_wheel` — digits that scroll like a mechanical odometer and land on the figure. Not to be confused with `counter`, which interpolates a value and rewrites the number (its glyphs jump). Le réglage se fait par `digits`, `duration` et `easing` sur le composant.
- `table` — tableau avec column_widths, column_align, cell_padding, show_borders

### UI Components
- `badge` — pill avec icon, dot indicator, pulse animation, count badge
- `avatar` / `avatar_group` — avatar circulaire / groupe empilé avec "+N"
- `switch` — toggle animé on/off avec toggle_at
- `slider` — curseur horizontal animé avec animate_to/animate_at
- `rating` — étoiles avec remplissage partiel animé
- `kbd` — touche clavier visuelle (effet 3D)
- `tooltip` — label flottant avec flèche directionnelle
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
- `success_check` — a checkmark that draws itself inside a halo, with a pop and a settling rotation
- `pointer` — a simulated **mouse** cursor (arrow + click ring) following waypoints. `cursor` is a text caret, not this. See [rules/pointer-walkthrough.md](.claude/skills/rustmotion/rules/pointer-walkthrough.md).

### Diagrammes
`arrow`, `connector`, `timeline`, `line`

> Pour faire suivre une trajectoire à un composant, utilise l'effet d'animation `motion_path` (données de chemin SVG, orientation optionnelle selon la tangente) plutôt que d'empiler des `translate`. Voir [rules/motion-path.md](.claude/skills/rustmotion/rules/motion-path.md).

### Média
`mockup`, `lottie`, `cursor`, `emitter`, `particle`, `qr_code`

> `emitter` — champ de particules radial avec naissance, trajet, mort et
> renaissance, en forme close (donc cherchable par `still`). Remplace `particle`,
> déprécié, dont les cinq presets figés n'ont aucun cycle de vie. Voir
> [rules/emitter-lifecycle.md](.claude/skills/rustmotion/rules/emitter-lifecycle.md).

### Audio
- `waveform` — visualisation d'onde audio réactive au volume de la piste
- `audio_spectrum` — barres de spectre audio réactives (FFT)

> Voir [rules/audio-reactive.md](.claude/skills/rustmotion/rules/audio-reactive.md) pour lier un composant à une piste `audio` via `style.audio-reactive`.

## Text finishes

Four mechanisms that each used to demand a hand-assembled sub-tree. Details and pitfalls in [rules/text-polish.md](.claude/skills/rustmotion/rules/text-polish.md).

- **`shimmer`** — animation effect: a band of light sweeps over the element. Composited `SrcATop` inside the node's layer, so it only lights up **pixels that are actually painted** (the glyphs, not the box). Combined with `char_blur_in`, this is "text stagger".
- **`text.states` + `text.swap`** — a label that becomes another one: the outgoing one rises while blurring, the incoming one rises from below while unblurring. The box is measured on the **longest** label, not the first one.
- **`text.caret`** — a caret (`line` or `block`) pinned to a `typewriter`'s reveal head. A `cursor` composited alongside would stay where it was placed while the text grows underneath it.
- **`pop_in`** — a two-beat entry preset: a back-out scale that *places* the element, then a short elastic pulse that draws the eye back to it.

The seven `char_*` presets are tuned via `direction` (up/down/left/right), `distance`, `scale_from`, `jitter`+`seed` (deterministic stagger irregularity) and `ink_from` (each unit's starting colour). See [rules/char-animation-tuning.md](.claude/skills/rustmotion/rules/char-animation-tuning.md) and, for tokens arriving, [rules/streaming-text.md](.claude/skills/rustmotion/rules/streaming-text.md).

> If you're asked for an effect named in the Hyperframes vocabulary ("streaming text", "number wheel", "badge pop", "top-down letters"…), check the mapping table first: [rules/hyperframes-mapping.md](.claude/skills/rustmotion/rules/hyperframes-mapping.md).

## Architecture

### Render Pipeline (CSS engine)

Le moteur utilise un pipeline **box_tree → layout_pass → paint_pass** inspiré des navigateurs web :

1. **box_tree** (`box_builder.rs`) — construit un arbre de `BoxNode { css: CssStyle, children, intrinsic }` depuis les composants JSON résolus
2. **layout_pass** (`engine/layout_pass.rs`) — orchestre taffy pour calculer les `BoxLayout { x, y, width, height }` de chaque nœud. Les feuilles avec un `IntrinsicMeasure` (texte, image, table) sont mesurées via une `measure_fn`.
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
└── rustmotion/src/
    ├── cli/                    # Le binaire `rustmotion` (clap + sous-commandes)
    │   └── commands/           # validate, render, schema, info
    ├── studio/                 # Le binaire `rustmotion-studio` (feature `studio`)
    ├── encode/                 # Encodeurs vidéo/audio, mux
    └── loader.rs               # Chargement JSON/HTML → ResolvedScenario
```

> Both binaries live in the published `rustmotion` crate: a crate with only a
> `[lib]` installs nothing executable through `cargo install`, and `cargo install
> --git <url>` **refuses** a repository where more than one package declares a
> `[[bin]]` ("multiple packages with binaries found" — neither `default-members`
> nor `required-features` changes that count). That is why the studio is a module
> of this crate rather than a package of its own: as a package it depended on
> `loader`/`encode`, so making it a dependency of `rustmotion` was a cycle, which
> cargo refuses even when optional.
>
> ```bash
> cargo install --git https://github.com/LeadcodeDev/rustmotion                     # CLI
> cargo install --git https://github.com/LeadcodeDev/rustmotion --features studio   # CLI + studio
> ```
>
> `studio` is out of the default build: it pulls gpui and a GUI toolchain, which
> do not compile everywhere the CLI does. The `rustmotion studio` subcommand
> opens the same studio as the `rustmotion-studio` binary, with the same `-f` /
> `-d`. Without the feature the subcommand still exists but stays out of
> `--help`, and running it answers with the `cargo install` line that grants it
> instead of "unrecognized subcommand".
> `--workspace` alone no longer compiles the studio: CI passes
> `--features rustmotion/studio` to clippy and to the tests, without which 11,600
> lines stop being checked while staying green.

### Aucun commentaire, nulle part

Le code Rust ne porte **aucun commentaire** — ni `//`, ni `///`, ni `//!`. La
règle ne valait que pour le studio ; elle vaut maintenant pour les quatre
crates. 16 355 commentaires ont été retirés d'un coup, et `//` comme `//!` sont
à zéro : en ajouter un, c'est rouvrir ce qui a été fermé.

Quand un commentaire semble nécessaire, c'est le signal qu'il faut **renommer
la liaison ou extraire une fonction nommée** : l'explication va dans un
identifiant, où elle ne peut pas diverger du code. Le raisonnement qui n'a
vraiment nulle part d'autre où vivre va dans le **message de commit**, la
description de PR ou l'issue.

#### La seule exception, et elle n'est pas de la documentation

Les 1 671 `///` restants sont du **texte d'interface**, pas des commentaires :

| Lu par | Où | Ce que ça produit |
|---|---|---|
| `clap` | types dérivant `Parser`/`Subcommand`/`Args`/`ValueEnum` | le texte de `rustmotion --help` |
| `schemars` | types dérivant `JsonSchema`, et leurs champs | les `description` du schéma exporté, que `validate --strict-attrs` consomme |

Les retirer n'enlèverait pas un commentaire, ça viderait `--help` et 1 223
descriptions du schéma. Ils ne vivent donc que **sur un type portant l'un de ces
derives, ou sur un de ses champs** — partout ailleurs, un `///` est un
commentaire, et il n'a pas sa place.

> Corollaire : docs.rs des quatre crates publiées est vide, et c'est assumé.
>
> Corollaire moins évident : un doctest vit dans un doc comment, donc supprimer
> celui-ci supprime le test. Les cinq qui existaient ont été portés en `#[test]`
> ordinaires plutôt que perdus. Écrire un exemple exécutable, désormais, c'est
> écrire un test.

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
