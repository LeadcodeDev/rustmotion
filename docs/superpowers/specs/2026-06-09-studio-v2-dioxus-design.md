# Studio v2 — Design (Dioxus app + Skia headless + annotation/inspector)

Date: 2026-06-09
Status: Approved design, pre-plan

## Contexte & problème

Le « mode studio » actuel (`rustmotion-studio`) n'est qu'une fenêtre de
preview : winit + softbuffer, UI dessinée à la main en Skia, lecture seule.
On veut le **revoir de 0** en une vraie application desktop (style open-slide) :
bibliothèque de scénarios, preview, **sélection d'éléments au clic**,
**inspecteur d'édition de propriétés**, et **boucle d'annotation→agent**.

Inspiration directe : open-slide (web app Vite). open-slide obtient sélection,
inspecteur lié aux propriétés et édition live « gratuitement » parce que les
slides **sont** le DOM (HTML/CSS dans un navigateur). Rustmotion ne peut pas
copier ça tel quel sans renoncer à son différenciateur.

## Différenciateur à préserver

Rustmotion existe face à Remotion parce qu'il **n'embarque pas de navigateur** :
léger, portable, binaire distribuable, rendu déterministe et contrôlé. Toute
solution qui rend la **vidéo** via une webview/Chromium reconstruit Remotion et
érode cet avantage. Décision : **garder Skia comme unique moteur de rendu vidéo**.

## Décision d'architecture (verrouillée)

- **Un seul moteur de rendu : Skia** dans `rustmotion-core`, headless,
  **zéro dépendance Dioxus**. Le studio l'affiche, ne le double pas
  → pas de dérive preview/sortie.
- **Dioxus est l'app du studio** (`dioxus-desktop`, backend webview/wry) :
  possède la fenêtre et toute l'UI. Isolé dans `rustmotion-studio`, derrière
  une **feature cargo `studio`** pour que les builds CI/headless restent légers
  et sans webview.
- **Transport des frames** : `use_asset_handler` (custom asset handler Dioxus
  desktop, streaming Rust→webview sans JS — cas d'usage documenté : vidéo).
  `<img src="/frame/{scene}/{idx}?v={rev}">`.
- **Interactivité = modèle overlay** : pas de second renderer. Le `paint_pass`
  Skia émet un **hit-map** (`rect + pointer + kind` par nœud) pour la frame
  courante ; Dioxus pose par-dessus l'`<img>` des `<div>` transparents
  cliquables, positionnés en **% des dimensions vidéo** → alignement automatique
  à toute taille/DPI.
- **Édition live** : un patch d'inspecteur mute le JSON (via JSON Pointer),
  invalide la frame, le moteur re-render en quelques ms (ce n'est pas un
  browser), `?v=rev` est bumpé, l'`<img>` re-fetch. Ressenti instantané.
- **CLI inchangé** ; `rustmotion studio` lance l'app Dioxus (gated `studio`).

### Pourquoi pas les alternatives

- **Double moteur** (Dioxus rend le JSON en preview, Skia rend la vidéo) :
  preview ≠ sortie (dérive), + réimplémentation des 51 composants + animation
  en HTML. Casse la prédictibilité. Rejeté.
- **Webview pour la vidéo** (Remotion-like) : abandonne le différenciateur.
  Hors scope (réévaluable si le différenciateur devient négociable).
- **Blitz (renderer natif wgpu) d'emblée** : expérimental, API d'embarquement
  de surface custom immature. Cible de migration future possible (le code RSX
  est largement portable), pas maintenant.

## Périmètre du studio v2

Niveau retenu : **bibliothèque + preview + inspecteur (édition de props)**
+ **annotation**. PAS d'édition timeline/composition (réordonner scènes,
keyframes caméra) en v2.

## Décomposition & ordre de construction

| # | Sous-projet | Livre | Dépend de |
|---|---|---|---|
| **B** | **Fondation** : Dioxus shell + frame-service + asset handler + canvas/overlay + playback/timeline + watch→signals | l'app tourne, on voit la vidéo, on scrub, on clique un élément | — |
| **A** | Bibliothèque de scénarios (sidebar, ouvrir/créer, thumbnails) | gestion multi-scénarios | B |
| **C** | Socle sélection (hit-map dans core + JSON Pointers) | référence élément fiable, partagée par D et E | partiellement dans B |
| **D** | Inspecteur / édition de props (binding bidirectionnel) | éditer typo/couleur/position live | B, C |
| **E** | Annotation (panneau + champ `annotations` + skill apply-then-validate) | boucle de feedback LLM | B, C |

On construit **B en premier** (elle porte le risque technique). A/C suivent
vite ; D et E en parallèle sur le socle. **Chaque sous-projet aura son propre
spec.** Ce document spécifie **B** en détail + le north star pour A/C/D/E.

## Fondation (sous-projet B) — détail

### Crates

- `rustmotion-core` (backbone, renderer-agnostic) :
  - `engine/paint_pass.rs` — émet un `HitMap` pour la frame courante pendant
    la descente top-down (rect en coords vidéo, ordre de peinture = z-order).
  - `engine/box_tree.rs` + `rustmotion-components/src/box_builder.rs` —
    `BoxNode` porte `source_path: Option<String>` (JSON Pointer) vers le
    `children` source ; threadé pendant la récursion.
  - `schema/scenario.rs` — champ top-level `annotations`
    (`#[serde(default, skip_serializing_if = "Vec::is_empty")]`), ignoré au
    rendu et par la passe geometry de `validate`.
- `rustmotion-studio` (Dioxus, feature `studio`) :
  - `frame_service.rs` — ex-`render_worker` promu service : thread Skia, cache
    `(scene, idx) → (png_bytes, HitMap)`, messages `Render`/`Invalidate`,
    lookup synchrone (render-on-miss) pour l'asset handler.
  - `asset.rs` — enregistrement `use_asset_handler("frame", …)`.
  - `watch.rs` — `notify` → push dans un signal (au lieu d'un redraw winit).
  - `app.rs` (composant racine Dioxus) + composants : `Canvas`, `Overlay`,
    `Timeline` (+ ultérieurement `Sidebar`, `Inspector`, `Comments`).
- `rustmotion-cli` — sous-commande `studio` lance l'app (gated `studio`).

### Structures clés

```rust
struct HitNode { rect: Rect, pointer: String, kind: String } // coords vidéo
type HitMap = Vec<HitNode>;                                   // ordre de peinture = z-order

// Contrat asset handler
// GET /frame/{scene}/{idx}   -> image/png (cache, render-on-miss)
// ?v={rev}                   -> cache-bust après édition

// État Dioxus (signals)
// scene, frame, rev, playing,
// selected: Option<String /* JSON Pointer */>,
// hitmap: Vec<HitPct /* rect en % */>
```

### Mapping coordonnées

Les rects du hit-map (coords vidéo) → **pourcentages** (`x/video_w`, `y/video_h`,
`w/video_w`, `h/video_h`). Overlay `position:absolute; inset:0` au-dessus de
l'`<img>` ; hotspots dimensionnés en %. img et overlay scalent du même facteur
→ alignement automatique, aucun calcul de scale manuel.

### Ce qui disparaît / ce qui survit

- **Disparaît** : `preview/app.rs` (fenêtre winit, event loop, softbuffer,
  souris/timeline main), `preview/ui.rs` (chrome dessiné en Skia). Leurs rôles
  (timeline, play/pause, scrub) sont **réexprimés en RSX/HTML/CSS**.
- **Survit / réutilisé** : `render_worker` (→ frame-service), cache de frames,
  watcher `notify`, et **`rustmotion-core` intact (zéro Dioxus)**.

## Spike de perf (en tête de B — porte de décision)

Mesures sur scénario réel, à **1180×2256** (res marketing) et **1920×1080** :
1. temps de render Skia / frame ;
2. temps d'encodage (PNG vs JPEG vs WebP) ;
3. round-trip asset handler + decode/affichage webview ;
4. fps soutenu en lecture (swap `<img src>`).

Seuils de décision :
- Édition (refresh après patch) **< ~50 ms** → instantané.
- Scrub (par seek) **< ~100 ms**.
- Lecture preview **≥ 24–30 fps**, sinon mitigations.

Mitigations classées : cache pré-rendu (déjà là) → JPEG/WebP au lieu de PNG →
preview **downscalée** (full-res seulement à l'export) → (dernier recours)
chemin texture wgpu/Blitz.

Sortie : **note de décision datée** — go webview tel quel / go avec mitigation X
/ reconsidérer Blitz.

## Stratégie de tests

- **Unit** : mapping rect→% ; resolve/round-trip d'un JSON Pointer sur le
  scénario ; serde `annotations` (default/skip) ; `validate` ignore `annotations`.
- **Smoke** : render une frame → `HitMap` non vide avec les `kind` attendus ;
  un point (x,y) résout vers le bon `pointer`.
- **Perf** : le spike ci-dessus = critère de validation de la fondation.

## North star pour les sous-projets ultérieurs

- **A — Bibliothèque** : modèle workspace (liste de fichiers/scénarios récents,
  ouvrir/créer), thumbnails = frames Skia par scène.
- **C — Socle sélection** : déjà amorcé dans B (hit-map + pointers) ; durcir la
  résolution clic→pointer et le format de référence.
- **D — Inspecteur** : table de binding props↔JSON par composant ; commencer par
  typographie / couleur / position. Patch via JSON Pointer + re-render.
- **E — Annotation** :
  - Données : champ `annotations` dans le scénario (versionné, strippable via
    `validate --fix` pour les `status:resolved`). Structure :
    `{ id, note, status: open|resolved, frame, view, scene,
       target: { pointer, kind, rect } }`.
  - Capture : clic élément (hit-map) + point temporel (= frame courante) + note
    (textarea) → append `annotations`. Panneau : liste, édition, suppression,
    statut, marqueurs timeline, surbrillance élément.
  - Application : skill **apply-then-validate** — lit `annotations` `open`,
    applique chaque edit ciblé, lance `rustmotion validate` (schema+geometry),
    marque `resolved` ; hot-reload montre le résultat.
  - « Agent watching » d'open-slide (pont live studio↔agent) = évolution future
    par-dessus la persistance `annotations`.

## Réécriture studio : éviter le piège boucle de reload

Quand le studio **écrit** le scénario (édition props, ajout d'annotation), le
watcher `notify` se redéclenche. Garde-fou : comparer un hash du scénario
**hors `annotations`** ; si la partie rendable est inchangée, ne rafraîchir que
l'état en mémoire (liste annotations / props) sans invalider le cache de frames.

## Nuances explicites

1. Cette fondation **ne corrige pas** la douleur de layout (dépassements /
   superpositions). C'est un **axe séparé**, à traiter dans le moteur
   (mesure de texte cosmic-text, couverture CSS, ergonomie schéma). L'éditeur
   n'en a pas besoin pour exister.
2. On rappelle qu'on est **déjà sur taffy** (vrai moteur flex/grid) : la douleur
   layout est probablement des causes locales réparables, pas un défaut
   fondamental justifiant une refonte navigateur.

## Hors scope v2

- Édition timeline/composition (réordonner scènes, durées, keyframes caméra).
- Migration Blitz (renderer natif).
- Rendu vidéo via webview.
