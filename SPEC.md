# Rustmotion v2 — Architecture Compositionnelle

## 1. Overview

**Vision :** Réécrire le moteur interne de rustmotion avec une architecture compositionnelle inspirée de Flutter/Ratatui/GPUI, où chaque comportement (layout, animation, style) est un trait indépendant implémenté par les composants, remplaçant les structs monolithiques actuelles avec 15 champs dupliqués.

**Problème :** L'architecture actuelle souffre de 3 défauts structurels :
1. Duplication massive — 10+ champs identiques copiés sur 13 structs de layers
2. Positionnement fragile — mix incohérent entre coordonnées absolues et layout flex/grid
3. Extensibilité coûteuse — ajouter un nouveau comportement oblige à modifier 11+ structs, leurs impls, et 4-5 match arms

**Solution :** Un système de composants (structs) avec enum-dispatch, où les comportements sont des traits composables. Le layout est flow-based par défaut (comme CSS normal flow), avec opt-in pour le positionnement absolu. Le JSON reste l'interface utilisateur.

**Utilisateurs cibles :** Les développeurs/créateurs utilisant rustmotion via JSON (souvent généré par LLM), et les contributeurs au moteur.

---

## 2. Goals & Non-Goals

### Goals
- **G1 :** Un trait par comportement — un composant gagne une capacité en implémentant un trait
- **G2 :** Flow layout par défaut — les enfants d'un conteneur sont positionnés automatiquement
- **G3 :** Positionnement absolu en opt-in — `"position": "absolute"` sort un élément du flow
- **G4 :** Enum-dispatch — un enum central `Component` dispatche vers des structs indépendantes
- **G5 :** Zéro duplication de champs — les champs communs vivent dans des structs de configuration réutilisées par composition
- **G6 :** API builder interne — chaque trait expose des méthodes builder (`.items_center()`, `.justify_content()`) pour la construction programmatique
- **G7 :** Parité fonctionnelle — tout ce que v1 pouvait rendre, v2 le peut aussi
- **G8 :** Le JSON reste l'interface utilisateur unique

### Non-Goals
- Rétrocompatibilité avec les scénarios JSON v1
- DSL Rust exposé aux utilisateurs (les builders sont internes)
- Hot-reloading de composants custom
- ECS (Entity Component System) — on reste sur un arbre, pas un monde plat
- Système de thème/style hérité (CSS cascading)

---

## 3. User Stories

### Utilisateur final (auteur JSON)
- *"En tant qu'auteur, je veux que mes éléments se positionnent automatiquement dans un conteneur flex, sans calculer des coordonnées x/y manuellement."*
- *"En tant qu'auteur, je veux pouvoir placer un élément en absolu quand j'en ai besoin, en ajoutant `"position": "absolute"` avec des coordonnées."*
- *"En tant qu'auteur, je veux que la scène elle-même soit un conteneur layout, pour centrer mes éléments verticalement et horizontalement sans wrapper Card."*

### Contributeur au moteur
- *"En tant que développeur, je veux ajouter un nouveau comportement (ex: `clip`, `border_radius`) en créant un trait + une struct de config, sans toucher à chaque composant."*
- *"En tant que développeur, je veux ajouter un nouveau type de composant en créant une struct + implémentant les traits pertinents, sans modifier 5 fonctions match."*

---

## 4. Functional Requirements

### 4.1 — Système de traits (Must)

Chaque comportement est un trait indépendant. Un composant implémente uniquement les traits pertinents.

#### Traits de base

| Trait | Responsabilité | Méthodes clés |
|---|---|---|
| `Widget` | Rendu et mesure | `render(canvas, ctx)`, `measure(constraints) -> Size` |
| `Animatable` | Animations, presets, wiggle, motion blur | `animation_config() -> &AnimationConfig` |
| `Timed` | Visibilité temporelle | `timing() -> (Option<f64>, Option<f64>)` |
| `Styled` | Opacité, padding, margin | `style_config() -> &StyleConfig` |

#### Traits de layout

| Trait | Responsabilité | Méthodes clés |
|---|---|---|
| `Container` | Contient des enfants, définit le layout | `children()`, `layout_config() -> &LayoutConfig` |
| `FlexContainer` | Layout flex spécifique | `direction()`, `justify()`, `align()`, `gap()`, `wrap()` |
| `GridContainer` | Layout grid spécifique | `template_columns()`, `template_rows()` |

#### Traits visuels (optionnels)

| Trait | Responsabilité | Méthodes clés |
|---|---|---|
| `Bordered` | Bordure | `border() -> Option<&Border>` |
| `Rounded` | Coins arrondis | `corner_radius() -> f32` |
| `Shadowed` | Ombre portée | `shadow() -> Option<&Shadow>` |
| `Backgrounded` | Fond (couleur/gradient) | `background() -> Option<&str>` |
| `Clipped` | Clip au bounds | `clip() -> bool` |

#### Acceptance Criteria
- Chaque trait est défini dans son propre fichier ou module `traits/`
- Un composant qui n'implémente pas un trait ne possède simplement pas la capacité — pas de `Option` partout
- Les traits ont des implémentations par défaut sensées quand applicable

---

### 4.2 — Structs de configuration partagées (Must)

Les champs communs sont factorisés dans des structs de configuration réutilisées par composition (pas par héritage).

```rust
/// Embedded in any component that supports animation
#[derive(Default)]
pub struct AnimationConfig {
    pub animations: Vec<Animation>,
    pub preset: Option<AnimationPreset>,
    pub preset_config: Option<PresetConfig>,
    pub wiggle: Option<Vec<WiggleConfig>>,
    pub motion_blur: Option<f32>,
}

/// Embedded in any component that supports timed visibility
#[derive(Default)]
pub struct TimingConfig {
    pub start_at: Option<f64>,
    pub end_at: Option<f64>,
}

/// Embedded in any component that supports style modifiers
pub struct StyleConfig {
    pub opacity: f32,          // default 1.0
    pub padding: Option<Spacing>,
    pub margin: Option<Spacing>,
}

/// Embedded in any component that is a flex container
pub struct FlexConfig {
    pub direction: Direction,  // Column by default
    pub justify: Justify,      // Start by default
    pub align: Align,          // Start by default
    pub gap: f32,              // 0 by default
    pub wrap: bool,            // false by default
}
```

#### Acceptance Criteria
- Chaque struct de config implémente `Default` avec des valeurs sensées
- Chaque struct de config implémente `Deserialize` pour être parsée directement depuis le JSON
- Un composant compose les configs dont il a besoin : `struct Text { ..., animation: AnimationConfig, timing: TimingConfig, style: StyleConfig }`

---

### 4.3 — Macro d'implémentation automatique (Should)

Pour éviter le boilerplate `impl Animatable for Text { fn animation_config(&self) -> &AnimationConfig { &self.animation } }` × N composants, fournir une macro :

```rust
impl_trait!(Text, Animatable, animation);
impl_trait!(Text, Timed, timing);
impl_trait!(Text, Styled, style);
// Ou groupé :
impl_traits!(Text {
    Animatable => animation,
    Timed => timing,
    Styled => style,
});
```

#### Acceptance Criteria
- La macro génère les impls de trait à partir du nom du champ
- Fonctionne pour tous les traits qui délèguent à une struct de config

---

### 4.4 — Enum `Component` avec dispatch (Must)

```rust
#[derive(Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Component {
    Text(Text),
    Shape(Shape),
    Image(Image),
    Icon(Icon),
    Svg(Svg),
    Video(Video),
    Gif(Gif),
    Codeblock(Codeblock),
    Counter(Counter),
    Caption(Caption),
    // Containers
    Stack(Stack),     // remplace Group — positionnement absolu des enfants
    Flex(Flex),       // remplace Card/Flex — layout flex
    Grid(Grid),       // layout grid
}
```

L'enum `Component` est le point d'entrée du dispatch. Des méthodes helper castent vers les traits :

```rust
impl Component {
    pub fn as_widget(&self) -> &dyn Widget { ... }
    pub fn as_animatable(&self) -> Option<&dyn Animatable> { ... }
    pub fn as_timed(&self) -> Option<&dyn Timed> { ... }
    pub fn as_styled(&self) -> Option<&dyn Styled> { ... }
    pub fn as_container(&self) -> Option<&dyn Container> { ... }
}
```

#### Acceptance Criteria
- Le match exhaustif est vérifié à la compilation
- Ajouter un composant = ajouter une struct + un variant + ses trait impls
- Le dispatch trait retourne `Option` pour les traits non-implémentés par un composant

---

### 4.5 — Système de layout (Must)

#### 4.5.1 — Flow layout (par défaut)

Tous les enfants d'un conteneur (`Flex`, `Grid`) participent au flow layout par défaut. Le protocole de layout suit le modèle Flutter :

1. **Constraints down** — le parent envoie des contraintes `(min_width, max_width, min_height, max_height)` à chaque enfant
2. **Size up** — chaque enfant retourne sa taille désirée dans ces contraintes
3. **Position** — le parent positionne chaque enfant selon son algorithme (flex, grid)

```rust
pub struct Constraints {
    pub min_width: f32,
    pub max_width: f32,
    pub min_height: f32,
    pub max_height: f32,
}

impl Constraints {
    pub fn unbounded() -> Self { ... }
    pub fn tight(width: f32, height: f32) -> Self { ... }
}
```

#### 4.5.2 — Positionnement absolu (opt-in)

Un enfant peut se sortir du flow via `"position": "absolute"` dans le JSON :

```json
{
  "type": "text",
  "content": "Overlay",
  "position": "absolute",
  "x": 100,
  "y": 200
}
```

Internement :

```rust
pub enum PositionMode {
    Flow,                        // default — participe au layout parent
    Absolute { x: f32, y: f32 }, // hors flow, coordonnées relatives au parent
}
```

Les enfants absolus sont rendus *après* les enfants flow (comme CSS), mais ne participent pas au calcul de taille du conteneur.

#### 4.5.3 — La scène comme conteneur racine

La `Scene` elle-même devient un conteneur layout implicite. Par défaut c'est un `Stack` (enfants empilés en absolu — rétrocompatible). L'auteur peut spécifier un layout :

```json
{
  "duration": 3.0,
  "layout": {
    "direction": "column",
    "justify": "center",
    "align": "center",
    "gap": 20
  },
  "layers": [...]
}
```

Si `layout` est présent, la scène se comporte comme un `Flex`. Sinon, comme un `Stack` (positionnement absolu libre).

#### Acceptance Criteria
- Le layout par défaut (sans `position` ni `layout` sur la scène) est rétrocompatible : les éléments sont positionnés à (0,0) ou à leur `x/y` absolu
- Un conteneur Flex positionne ses enfants automatiquement
- `position: absolute` fonctionne dans n'importe quel conteneur
- Les contraintes se propagent correctement (un Flex dans un Flex)

---

### 4.6 — Pipeline de rendu (Must)

Le rendu se fait en deux passes :

#### Pass 1 — Layout
```
layout_tree(root: &Component, constraints: Constraints) -> LayoutTree
```
Parcourt l'arbre de composants top-down, calcule la taille et la position de chaque noeud. Produit un `LayoutTree` — un arbre parallèle avec les positions/tailles résolues.

```rust
pub struct LayoutNode {
    pub position: (f32, f32),    // position résolue dans l'espace parent
    pub size: (f32, f32),        // taille résolue
    pub children: Vec<LayoutNode>,
}
```

#### Pass 2 — Render
```
render_tree(canvas: &Canvas, component: &Component, layout: &LayoutNode, ctx: &RenderContext)
```
Parcourt les deux arbres en parallèle. Pour chaque noeud :
1. Appliquer les transforms canvas (position, animation, opacity)
2. Appeler `widget.render(canvas, ctx)` pour le contenu spécifique
3. Récurser dans les enfants

```rust
pub struct RenderContext {
    pub time: f64,
    pub scene_duration: f64,
    pub video_config: VideoConfig,
    pub frame_index: u32,
}
```

#### Acceptance Criteria
- Le layout est calculé une seule fois par frame (pas à chaque render récursif)
- Les animations de `translate_x/y` ne modifient pas le layout — elles sont appliquées au render seulement
- Les animations de `scale` ne modifient pas le layout (même principe que CSS `transform`)

---

### 4.7 — Parsing JSON → Composants (Must)

Le JSON est parsé via `serde` comme aujourd'hui (`#[serde(tag = "type")]`). Le champ `position` a un traitement spécial :

```rust
// Serde intermédiaire pour le parsing
#[derive(Deserialize)]
#[serde(untagged)]
pub enum PositionField {
    Mode(String),           // "absolute"
    Coordinates { x: f32, y: f32 }, // pour rétrocompat
}
```

Si `"position": "absolute"` → `PositionMode::Absolute` avec `x`/`y` lus séparément.
Si `"position": { "x": 100, "y": 200 }` → `PositionMode::Absolute { x: 100, y: 200 }`.
Si absent → `PositionMode::Flow`.

#### Acceptance Criteria
- `serde_json::from_str::<Scenario>()` parse l'arbre complet en un seul appel
- Pas de phase de transformation post-parse — les structs sont directement utilisables
- Les erreurs de parse sont claires et indiquent le chemin JSON fautif

---

### 4.8 — Composants concrets (Must)

#### Leaf components (pas d'enfants)

| Component | Traits implémentés | Champs spécifiques |
|---|---|---|
| `Text` | Widget, Animatable, Timed, Styled | content, font_size, color, font_family, font_weight, font_style, align, max_width, line_height, letter_spacing, shadow, stroke, background |
| `Shape` | Widget, Animatable, Timed, Styled, Bordered, Rounded | shape, size, fill, stroke, text |
| `Image` | Widget, Animatable, Timed, Styled | src, size, fit |
| `Icon` | Widget, Animatable, Timed, Styled | icon, color, size |
| `Svg` | Widget, Animatable, Timed, Styled | src, data, size |
| `Video` | Widget, Animatable, Timed, Styled | src, size, trim_start, trim_end, playback_rate, fit, volume, loop_video |
| `Gif` | Widget, Animatable, Timed, Styled | src, size, fit, loop_gif |
| `Counter` | Widget, Animatable, Timed, Styled | from, to, decimals, separator, prefix, suffix, easing, font_size, color, ... |
| `Caption` | Widget, Styled | words, font_size, color, active_color, background, style, max_width |
| `Codeblock` | Widget, Animatable, Timed, Styled, Rounded | code, language, theme, size, font_family, ..., reveals, states |

#### Container components

| Component | Traits implémentés | Champs spécifiques |
|---|---|---|
| `Stack` | Widget, Container, Animatable, Timed, Styled | layers (enfants en absolu) |
| `Flex` | Widget, Container, FlexContainer, Animatable, Timed, Styled, Bordered, Rounded, Shadowed, Backgrounded, Clipped | direction, justify, align, gap, wrap, layers, size |
| `Grid` | Widget, Container, GridContainer, Animatable, Timed, Styled, Bordered, Rounded, Shadowed, Backgrounded, Clipped | template_columns, template_rows, layers, size |

#### Acceptance Criteria
- Chaque composant a sa propre struct dans un fichier dédié (`components/text.rs`, `components/shape.rs`, ...)
- La liste des traits implémentés est le contrat public de chaque composant
- Un composant ne connaît pas les autres composants — le dispatch est dans l'enum

---

### 4.9 — Per-child flex/grid metadata (Must)

Comme aujourd'hui avec `CardChild`, les enfants d'un conteneur flex/grid peuvent avoir des propriétés de layout par-enfant :

```rust
pub struct Child {
    #[serde(flatten)]
    pub component: Component,
    // Flex
    pub flex_grow: Option<f32>,
    pub flex_shrink: Option<f32>,
    pub flex_basis: Option<f32>,
    pub align_self: Option<Align>,
    // Grid
    pub grid_column: Option<GridPlacement>,
    pub grid_row: Option<GridPlacement>,
    // Position
    #[serde(default)]
    pub position_mode: PositionMode,
}
```

---

## 5. Non-Functional Requirements

### Performance
- Le rendu ne doit pas régresser en performance par rapport à v1
- Enum dispatch (pas de vtable) pour le hot path `Component::as_widget().render()`
- Le layout pass doit être O(n) en nombre de composants (pas de re-layout récursif)
- Les caches d'assets (DashMap) sont conservés tel quel

### Maintenabilité
- Ajouter un nouveau composant = 1 fichier + 1 variant enum + trait impls
- Ajouter un nouveau trait/comportement = 1 fichier trait + impls sur les composants pertinents (pas tous)
- Moins de 100 lignes de match arms centralisés (contre ~300 aujourd'hui)

### Testabilité
- Les traits permettent de tester chaque comportement isolément
- Le layout peut être testé sans rendu Skia (juste les positions)
- `measure()` et `layout()` sont des fonctions pures

---

## 6. Architecture Technique

### Structure de fichiers cible

```
src/
├── main.rs                    # CLI (inchangé structurellement)
├── tui.rs                     # TUI progress (inchangé)
├── schema/
│   ├── mod.rs
│   ├── scenario.rs            # Scenario, Scene, VideoConfig, AudioTrack
│   ├── animation.rs           # Animation, Keyframe, Preset (inchangé)
│   └── types.rs               # Size, Spacing, Fill, Gradient, Stroke, ...
├── components/
│   ├── mod.rs                 # enum Component + dispatch helpers
│   ├── text.rs
│   ├── shape.rs
│   ├── image.rs
│   ├── icon.rs
│   ├── svg.rs
│   ├── video.rs
│   ├── gif.rs
│   ├── codeblock.rs
│   ├── counter.rs
│   ├── caption.rs
│   ├── stack.rs
│   ├── flex.rs
│   └── grid.rs
├── traits/
│   ├── mod.rs
│   ├── widget.rs              # Widget trait
│   ├── animatable.rs          # Animatable trait + AnimationConfig
│   ├── timed.rs               # Timed trait + TimingConfig
│   ├── styled.rs              # Styled trait + StyleConfig
│   ├── container.rs           # Container, FlexContainer, GridContainer
│   ├── bordered.rs
│   ├── rounded.rs
│   ├── shadowed.rs
│   ├── backgrounded.rs
│   └── clipped.rs
├── layout/
│   ├── mod.rs
│   ├── constraints.rs         # Constraints struct
│   ├── tree.rs                # LayoutNode, LayoutTree
│   ├── flex.rs                # Flex layout algorithm
│   ├── grid.rs                # Grid layout algorithm
│   └── stack.rs               # Stack layout (absolute positioning)
├── engine/
│   ├── mod.rs
│   ├── renderer.rs            # render_frame, render_tree (simplifié)
│   ├── animator.rs            # (inchangé)
│   ├── codeblock.rs           # (adapté pour utiliser les traits)
│   └── transition.rs          # (inchangé)
├── encode/
│   ├── mod.rs
│   ├── video.rs               # (inchangé structurellement)
│   └── audio.rs               # (inchangé)
└── macros.rs                  # impl_traits!() macro
```

### Choix techniques

| Décision | Choix | Justification |
|---|---|---|
| Dispatch | Enum (`Component`) | Pattern matching exhaustif, pas de vtable, inlining possible |
| Trait access | `Option<&dyn Trait>` via helpers sur l'enum | Permet de vérifier si un composant supporte un comportement sans match |
| Layout | 2 passes séparées (layout puis render) | Découple le positionnement du rendu, testable indépendamment |
| Position mode | Enum `Flow / Absolute` | Simple, explicite, pas d'ambiguïté |
| Config structs | Composition (embedded fields) | Pas de macro derive complexe, clair et explicite |
| Macro | `macro_rules!` simple | Pas besoin de proc-macro, le boilerplate est minimal et régulier |

---

## 7. Constraints & Assumptions

### Constraints
- Le JSON reste le seul format d'entrée
- Pas de breaking change sur les dépendances (Skia, resvg, openh264, rayon)
- L'animation et le rendu restent synchrones (pas de runtime async)
- Le rendu parallèle via rayon est conservé (frames indépendantes)

### Assumptions
- Le modèle Flutter constraints-down/size-up est suffisant pour les besoins de rustmotion
- Les performances de l'enum dispatch sont équivalentes aux match arms actuels
- L'API Iconify reste disponible et stable

---

## 8. Risks & Mitigations

| Risque | Impact | Probabilité | Mitigation |
|---|---|---|---|
| Régression du layout flex/grid (edge cases) | Haut | Moyen | Créer une suite de scénarios JSON de référence *avant* la migration, comparer pixel-perfect |
| Scope creep — vouloir tout refactorer d'un coup | Haut | Haut | Milestones stricts, chaque milestone produit un binaire fonctionnel |
| Complexité du parsing `position` (flow vs absolute) | Moyen | Moyen | Prototype le serde untagged enum tôt pour valider |
| Performance du layout pass ajouté | Bas | Bas | Le layout est O(n), les composants sont peu nombreux par scène |
| Macro `impl_traits!` trop rigide | Bas | Moyen | Fallback sur impls manuelles si la macro ne couvre pas un cas |

---

## 9. Success Metrics

| Métrique | Cible |
|---|---|
| Lignes de code dupliquées entre composants | < 5 (vs ~150 actuellement) |
| Lignes de match arms centralisés | < 100 (vs ~300 actuellement) |
| Fichiers à modifier pour ajouter un composant | 2 (struct + enum variant) vs 6 actuellement |
| Fichiers à modifier pour ajouter un comportement | 1 trait + N impls ciblés vs 11+ structs |
| Tests de layout | > 20 tests unitaires couvrant flex, grid, absolu, imbriqué |
| Parité fonctionnelle | Tous les exemples v1 (`showcase.json`, `presets_all.json`) rendus identiquement |

---

## 10. Resolved Questions

1. **Scène** — La scène reste `Scene` (pas un composant). Une composition (`Scenario`/`Video`) contient N scènes, chaque scène contient N composants. La scène a un champ `layout` optionnel pour configurer le layout de ses enfants.
2. **Grid** — Implémenté immédiatement avec le layout engine, pas différé.
3. **Codeblock** — Migré en dernier comme composant à part, vu sa complexité (1 134 lignes, states, reveal, cursor).

---

## 11. Milestones

### Milestone Roadmap

| #  | Name                        | Goal                                                            | Effort | Dependencies |
|----|-----------------------------|-----------------------------------------------------------------|--------|--------------|
| M1 | Foundation                  | Traits, config structs, macro, structure de fichiers             | M      | None         |
| M2 | Layout Engine               | Constraints, flex, grid, stack — testable sans rendu             | L      | M1           |
| M3 | Leaf Components             | Text, Shape, Image, Icon, Svg, Video, Gif, Counter, Caption     | L      | M1           |
| M4 | Containers + Scene          | Stack, Flex, Grid, Scene comme conteneur racine                 | M      | M1, M2       |
| M5 | Render Pipeline             | 2-pass (layout → render), animations, transitions, encodage     | L      | M2, M3, M4   |
| M6 | Codeblock                   | Migration du composant Codeblock                                | M      | M5           |
| M7 | Polish                      | Validation, docs, schema JSON, nettoyage code v1                | S      | M5           |

**Effort total estimé : XL** (M2 et M3 peuvent être parallélisés)

---

### Milestone 1 : Foundation

**Goal :** Poser la structure du projet v2 — tous les traits, les config structs, la macro, l'enum `Component`, sans aucun rendu.

**Deliverables :**
- `src/traits/` — Tous les traits définis : `Widget`, `Animatable`, `Timed`, `Styled`, `Container`, `FlexContainer`, `GridContainer`, `Bordered`, `Rounded`, `Shadowed`, `Backgrounded`, `Clipped`
- `src/traits/widget.rs` — Trait `Widget` avec `render()` et `measure()` + `RenderContext` struct
- `src/traits/animatable.rs` — Trait `Animatable` + `AnimationConfig` struct
- `src/traits/timed.rs` — Trait `Timed` + `TimingConfig` struct
- `src/traits/styled.rs` — Trait `Styled` + `StyleConfig` struct
- `src/traits/container.rs` — Traits `Container`, `FlexContainer`, `GridContainer` + `FlexConfig`, `GridConfig`
- `src/traits/{bordered,rounded,shadowed,backgrounded,clipped}.rs` — Traits visuels optionnels
- `src/components/mod.rs` — Enum `Component` avec tous les variants (structs vides/minimales)
- `src/components/mod.rs` — Helpers `as_widget()`, `as_animatable()`, `as_timed()`, `as_styled()`, `as_container()`
- `src/macros.rs` — Macro `impl_traits!()` pour déléguer les impls de trait aux champs de config
- `src/schema/scenario.rs` — `Scenario`, `Scene` (avec `layout` optionnel), `VideoConfig`, `AudioTrack`
- `src/schema/types.rs` — Types partagés : `Size`, `Spacing`, `Fill`, `Gradient`, `Stroke`, `PositionMode`, `Direction`, `Justify`, `Align`, ...
- `src/layout/constraints.rs` — Struct `Constraints`
- `src/layout/tree.rs` — Struct `LayoutNode`

**Key Tasks :**
- Créer l'arborescence de fichiers `traits/`, `components/`, `layout/`
- Définir chaque trait avec ses méthodes et types associés
- Définir les config structs avec `Default` et `Deserialize`
- Écrire la macro `impl_traits!()`
- Déclarer l'enum `Component` avec les variants (structs peuvent être des placeholders)
- Migrer `Scenario`, `Scene`, `VideoConfig` depuis `schema/video.rs` vers `schema/scenario.rs`
- Extraire les types partagés dans `schema/types.rs`

**Dependencies :** Aucune

**Acceptance Criteria :**
- `cargo build` compile sans erreur (le code v1 peut encore coexister temporairement)
- Tous les traits sont définis avec les signatures correctes
- La macro `impl_traits!()` fonctionne sur au moins un composant de test
- L'enum `Component` a ses 13 variants

**Estimated Effort :** M — Structure et signatures, peu de logique

**Risks :** Le design des traits peut nécessiter des ajustements au moment de l'implémentation réelle (M3). Mitigation : les traits restent simples et focalisés sur un seul comportement.

---

### Milestone 2 : Layout Engine

**Goal :** Implémenter le layout engine (flex, grid, stack) comme un module indépendant testable sans Skia.

**Deliverables :**
- `src/layout/flex.rs` — Algorithme de layout flex complet (direction, justify, align, gap, wrap, grow/shrink/basis, align_self)
- `src/layout/grid.rs` — Algorithme de layout grid (template columns/rows, fr/px/auto, placement, auto-placement)
- `src/layout/stack.rs` — Layout stack (positionnement absolu simple)
- `src/layout/mod.rs` — Fonction publique `compute_layout(scene, video_config) -> LayoutTree`
- Tests unitaires pour chaque algorithme

**Key Tasks :**
- Implémenter `Constraints` avec les méthodes utilitaires (`tight()`, `unbounded()`, `loosen()`, `constrain()`)
- Porter l'algo flex depuis `renderer.rs:compute_flex_layout()` (~100 lignes) en l'adaptant au protocole constraints
- Porter l'algo grid depuis `renderer.rs:compute_grid_layout()` (~180 lignes)
- Implémenter le layout stack (trivial — chaque enfant à sa position absolue)
- Gérer `PositionMode::Flow` vs `PositionMode::Absolute` dans chaque algo
- Gérer la récursion : un Flex contenant un Grid contenant des Text
- Écrire les tests unitaires :
  - Flex column : 3 textes empilés verticalement
  - Flex row : 3 éléments côte à côte avec gap
  - Flex justify : center, space-between, space-evenly
  - Flex align : center, stretch
  - Flex wrap : éléments qui dépassent la largeur
  - Flex grow/shrink : distribution de l'espace libre
  - Grid : 2 colonnes fr, 3 enfants
  - Grid : placement explicite (grid_column, grid_row)
  - Stack : 2 enfants en absolu
  - Mixte : enfant absolu dans un Flex
  - Imbrication : Flex > Flex > Text

**Dependencies :** M1 (traits `Container`, `FlexContainer`, `GridContainer`, struct `Constraints`, `LayoutNode`)

**Acceptance Criteria :**
- Tous les tests de layout passent
- Le layout engine ne dépend pas de Skia (pas d'import `skia_safe`)
- Le layout produit des positions et tailles correctes pour tous les cas ci-dessus
- Les enfants `position: absolute` sont ignorés dans le calcul de taille du parent

**Estimated Effort :** L — L'algo flex/grid est le coeur technique, beaucoup d'edge cases

**Risks :** Régression par rapport au layout v1 sur des edge cases. Mitigation : porter les algos existants comme base, puis adapter.

---

### Milestone 3 : Leaf Components

**Goal :** Implémenter toutes les structs de composants feuille avec leurs trait impls et leurs fonctions de rendu.

**Deliverables :**
- `src/components/text.rs` — Struct `Text` + impls `Widget`, `Animatable`, `Timed`, `Styled` + `render()` + `measure()`
- `src/components/shape.rs` — Struct `Shape` + impls + render/measure
- `src/components/image.rs` — Struct `ImageComp` + impls + render/measure
- `src/components/icon.rs` — Struct `Icon` + impls + render/measure (fetch Iconify)
- `src/components/svg.rs` — Struct `SvgComp` + impls + render/measure
- `src/components/video.rs` — Struct `VideoComp` + impls + render/measure
- `src/components/gif.rs` — Struct `GifComp` + impls + render/measure
- `src/components/counter.rs` — Struct `Counter` + impls + render/measure
- `src/components/caption.rs` — Struct `Caption` + impls + render/measure

**Key Tasks :**
- Pour chaque composant :
  1. Définir la struct avec ses champs spécifiques + config structs composées
  2. Implémenter `Deserialize` via serde (champs flattened pour les configs)
  3. Utiliser `impl_traits!()` pour les trait impls déléguées
  4. Implémenter `Widget::render()` en portant la fonction render_xxx correspondante depuis `renderer.rs`
  5. Implémenter `Widget::measure()` en portant le bras correspondant depuis `measure_layer()`
- Adapter les fonctions de rendu pour accepter le `RenderContext` au lieu de params individuels
- Conserver les caches d'assets (`ASSET_CACHE`, `GIF_CACHE`, `VIDEO_FRAME_CACHE`) et les fonctions `prefetch_icons()`, `preextract_video_frames()`

**Dependencies :** M1 (traits, config structs, macro)

**Acceptance Criteria :**
- Chaque composant a un `render()` et un `measure()` fonctionnels
- Les structs sont parsables depuis JSON via `serde`
- `render()` produit le même rendu visuel que les fonctions v1

**Estimated Effort :** L — 9 composants à porter, chacun avec rendu Skia spécifique (~1 500 lignes de render code à migrer)

**Risks :** Code de rendu Skia étroitement couplé au contexte actuel (ex: `render_counter` crée un `TextLayer` temporaire). Mitigation : adapter au cas par cas, le Counter peut appeler le render de Text via le trait `Widget`.

---

### Milestone 4 : Containers + Scene

**Goal :** Implémenter les composants conteneur et transformer la Scene en conteneur racine.

**Deliverables :**
- `src/components/stack.rs` — Struct `Stack` + impls `Widget`, `Container`, `Animatable`, `Timed`, `Styled`
- `src/components/flex.rs` — Struct `Flex` + impls `Widget`, `Container`, `FlexContainer`, `Animatable`, `Timed`, `Styled`, `Bordered`, `Rounded`, `Shadowed`, `Backgrounded`, `Clipped`
- `src/components/grid.rs` — Struct `Grid` + impls (mêmes traits que Flex + `GridContainer`)
- `src/schema/scenario.rs` — `Scene` avec champ `layout: Option<FlexConfig>` et `layers: Vec<Child>`
- `src/components/mod.rs` — `Child` struct avec per-child flex/grid metadata

**Key Tasks :**
- Implémenter Stack : render = itérer et rendre les enfants à leur position absolue
- Implémenter Flex : render = fond + bordure + ombre + clip + rendre les enfants aux positions layout
- Implémenter Grid : même pattern que Flex avec grid layout
- Implémenter `Child` wrapper avec `flex_grow`, `flex_shrink`, `align_self`, `grid_column`, `grid_row`
- Scene : si `layout` est présent, se comporter comme un Flex ; sinon comme un Stack
- Rendre les enfants récursivement (un Flex dans un Grid dans un Stack)

**Dependencies :** M1 (traits Container, FlexContainer, GridContainer), M2 (layout engine)

**Acceptance Criteria :**
- Un JSON avec `{ "type": "flex", "direction": "row", "layers": [...] }` produit un layout correct
- Les enfants `position: absolute` sont positionnés correctement dans n'importe quel conteneur
- L'imbrication fonctionne (Flex > Flex > Text)
- Le fond, bordure, ombre, clip fonctionnent sur Flex/Grid

**Estimated Effort :** M — Beaucoup de la logique vient du layout engine (M2), ici c'est surtout le wiring

**Risks :** L'interaction entre le layout engine et le rendu Skia (clip, transforms). Mitigation : tester visuellement avec des scénarios simples.

---

### Milestone 5 : Render Pipeline

**Goal :** Assembler le pipeline complet : JSON → parse → layout → render → encode. Remplacer le renderer v1.

**Deliverables :**
- `src/engine/renderer.rs` — Réécrit : `render_frame()` fait layout puis render_tree
- `render_tree()` — Parcourt `Component` + `LayoutNode` en parallèle, applique transforms/animations/opacity
- Intégration de l'animator existant (`resolve_animations`, `apply_wiggles`, motion blur)
- Intégration des transitions existantes (inchangées — opèrent sur les pixels RGBA)
- `src/encode/video.rs` — Appelle le nouveau `render_frame()` (l'interface ne change pas)
- Suppression complète du code v1 (`schema/video.rs` layers, ancien `renderer.rs`)

**Key Tasks :**
- Réécrire `render_frame()` :
  1. Construire l'arbre layout depuis la Scene
  2. `compute_layout(scene, video_config) -> LayoutTree`
  3. Créer le canvas Skia, clear background
  4. `render_tree(canvas, &scene.layers, &layout_tree, &render_ctx)`
- Réécrire `render_tree()` :
  1. Pour chaque (component, layout_node) :
     - Vérifier timing (`as_timed()`)
     - Résoudre animations (`as_animatable()` → `resolve_animations()`)
     - Appliquer wiggles
     - canvas.save()
     - Appliquer transforms (translate, scale, rotate, opacity, blur) — même logique qu'actuel
     - Appliquer margin/padding (`as_styled()`)
     - `component.as_widget().render(canvas, ctx)`
     - Si `as_container()` → render_tree récursif sur les enfants
     - canvas.restore()
- Conserver motion blur (multi-sampling inchangé)
- Intégrer `preextract_video_frames()` et `prefetch_icons()` avant la boucle
- Vérifier que les 4 fonctions d'encodage (`encode_video`, `encode_png_sequence`, `encode_gif`, `encode_with_ffmpeg`) fonctionnent

**Dependencies :** M2 (layout), M3 (leaf components), M4 (containers)

**Acceptance Criteria :**
- `cargo build` sans erreur ni warning
- `rustmotion render examples/showcase.json -o test.mp4` produit une vidéo
- `rustmotion render examples/presets_all.json -o test.mp4` — toutes les animations fonctionnent
- `rustmotion validate` fonctionne
- `rustmotion schema` génère le nouveau JSON schema
- Watch mode fonctionne
- L'ancien code v1 est entièrement supprimé

**Estimated Effort :** L — C'est le milestone d'intégration, beaucoup de debugging et d'edge cases

**Risks :** Régressions visuelles difficiles à détecter. Mitigation : comparer frame-by-frame les sorties v1 vs v2 sur les exemples existants.

---

### Milestone 6 : Codeblock Component

**Goal :** Migrer le composant Codeblock (1 134 lignes) vers la nouvelle architecture.

**Deliverables :**
- `src/components/codeblock.rs` — Struct `Codeblock` + trait impls + render complet
- Support complet : syntect highlighting, chrome, line numbers, highlights, reveal (typewriter, line-by-line), states (diffing animé), cursor blink

**Key Tasks :**
- Extraire la struct `Codeblock` avec ses champs spécifiques
- Implémenter `Widget::render()` en portant `render_codeblock()` depuis `engine/codeblock.rs`
- Implémenter `Widget::measure()` (taille du codeblock avec chrome, padding, line count)
- Adapter les sub-structs (`CodeblockChrome`, `CodeblockReveal`, `CodeblockState`, `CodeblockCursor`, `CodeblockHighlight`) dans le nouveau schema
- Supprimer `engine/codeblock.rs`

**Dependencies :** M5 (pipeline de rendu fonctionnel)

**Acceptance Criteria :**
- `examples/codeblock.json` produit le même rendu qu'en v1
- Typewriter reveal fonctionne
- State diffing animé fonctionne
- Cursor blink fonctionne
- Syntax highlighting avec les themes custom fonctionne

**Estimated Effort :** M — Code complexe mais isolé, le port est mécanique

**Risks :** Le codeblock accède directement au canvas Skia avec beaucoup de logique de mesure de texte. Mitigation : le porter tel quel dans `Widget::render()`, sans chercher à le refactorer.

---

### Milestone 7 : Polish

**Goal :** Validation, documentation, nettoyage.

**Deliverables :**
- `src/main.rs` — `validate_scenario()` réécrit pour les nouveaux types
- `README.md` — Mise à jour avec la nouvelle architecture et les nouveaux types JSON
- `SKILL.md` — Mise à jour
- `prompts/gemini.md` — Mise à jour
- JSON Schema — `rustmotion schema` génère le schema correct
- Suppression de tout fichier mort du code v1
- Exemples JSON mis à jour

**Key Tasks :**
- Réécrire la validation sémantique pour les nouveaux types
- Valider le format `position` (flow vs absolute)
- Mettre à jour toute la documentation avec les nouveaux noms de composants et la syntaxe de layout
- Mettre à jour les exemples JSON
- Vérifier que `schemars` génère un schema cohérent pour les nouveaux types
- Run final : tous les exemples se rendent correctement

**Dependencies :** M5 (pipeline complet), M6 si on veut inclure Codeblock dans les docs

**Acceptance Criteria :**
- `rustmotion validate` détecte les erreurs dans les nouveaux types
- `rustmotion schema` produit un JSON schema valide et complet
- README, SKILL.md, gemini.md sont à jour
- Tous les exemples se rendent sans erreur
- `cargo build` zero warnings

**Estimated Effort :** S — Principalement de la documentation et du cleanup

**Risks :** Aucun risque technique significatif.
