# Rule: `shatter` — fragments de Voronoi qui s'envolent (ou s'assemblent)

`shatter` (`style.animation`) découpe le rendu déjà peint d'un nœud — fond, bordure, enfants, tout son sous-arbre — en cellules polygonales déterministes (partition de Voronoi) et envoie chaque morceau voler loin d'un point d'origine, avec sa propre rotation et son propre fondu. C'est la brique pour une carte, une miniature ou une vitre qui explose en éclats et révèle ce qu'il y a derrière — voir issue #378.

## La forme

```json
{
  "type": "div",
  "style": {
    "width": 400, "height": 300,
    "background": "#1B1F3B",
    "animation": [{
      "name": "shatter",
      "delay": 1.2,
      "duration": 0.7,
      "mode": "out",
      "pieces": 24,
      "seed": 7,
      "origin": { "x": 0.5, "y": 0.5 },
      "spread": 1.0,
      "spin": 90,
      "depth": 0.4,
      "fade": true
    }]
  }
}
```

| Champ | Rôle | Défaut |
|---|---|---|
| `delay` | Attente avant que les éclats commencent à bouger (s) | `0` |
| `duration` | Durée de la dispersion (ou de l'assemblage en `mode: "in"`) (s) | `0.6` |
| `mode` | `"out"` / `"in"` / `"hold"` — voir plus bas | `"out"` |
| `pieces` | Nombre de cellules de Voronoi (borné en interne à `1..=64`) | `24` |
| `seed` | Graine de la partition et du jitter par éclat (direction, spin, depth) | `0` |
| `origin` | Point dont les éclats s'éloignent (ou vers lequel ils convergent en `"in"`), fraction `0..1` de la boîte du nœud — pas des px | `{ "x": 0.5, "y": 0.5 }` |
| `spread` | Multiplicateur du trajet radial à dispersion complète, relatif à la diagonale du nœud | `1.0` |
| `spin` | Rotation max en degrés à dispersion complète ; signe et amplitude tirés par éclat depuis `seed` | `90` |
| `depth` | Modulation d'échelle par éclat à dispersion complète — même sémantique « 0 = aucune » que `OrbitConfig.depth` (certains éclats grossissent, d'autres rétrécissent) | `0.4` |
| `fade` | Fait tomber l'opacité de chaque éclat à zéro à pleine dispersion (et l'inverse en `mode: "in"`) | `true` |

## `mode` décide quelle extrémité est le nœud intact

- **`"out"`** (défaut) : assemblé à `delay`, dispersé à `delay + duration`. **En dehors de cette fenêtre, l'effet ne contribue rigoureusement rien** — le nœud est pixel pour pixel identique à un nœud sans `shatter` dans sa liste `animation`. C'est le même court-circuit dur que `chromatic_aberration` et `zoom_blur` (voir [chromatic-aberration.md](chromatic-aberration.md)) plutôt qu'un fondu qui ne fait que s'approcher de zéro.
- **`"in"`** est le miroir : dispersé à `delay`, assemblé à `delay + duration` — et, comme `"out"`, en dehors de sa fenêtre le nœud se rend comme si l'effet était absent. La différence entre les deux modes n'est donc **pas** l'état aux bornes (les deux sont « pas d'effet » avant `delay` et après `delay + duration`) mais le sens dans lequel `progress` parcourt la fenêtre : `0` (assemblé) → `1` (dispersé) en `"out"`, l'inverse en `"in"`.
- **`"hold"`** joue la même dispersion que `"out"` mais se **fige** à pleine dispersion une fois `delay + duration` atteint, au lieu de revenir au nœud plein — il ne reconverge jamais. C'est le seul des trois modes dont l'état final diffère d'un nœud sans l'effet.

Piège à ne pas reproduire ailleurs : ne pas confondre « en dehors de la fenêtre » avec `progress` proche de 0 ou 1. `active_shatter` (`paint_pass.rs`) renvoie `None` — pas `Some(0.0)` ou `Some(1.0)` — hors fenêtre ; c'est un branchement de code différent (`paint_node_visual` direct, sans aucune rasterisation ni découpe), pas la même fonction évaluée à une borne.

## Comment c'est peint

Le sous-arbre du nœud est peint une seule fois dans une surface raster **dédiée**, à la taille de sa propre boîte (`box_layout.width × height`, coordonnées locales — même geste que `paint_inflated_material`/`silhouette_alpha_field` pour rasteriser puis relire des pixels). Cette capture désactive la hit-map (`PaintContext.hits: None`) : pendant que le nœud est fragmenté, ses enfants ne sont pas des cibles de clic cohérentes — seul le nœud lui-même reste cliquable, à son rectangle d'origine, exactement comme s'il n'était pas en train de se briser.

La partition de Voronoi vient d'un semis de points sur une grille approximative (`√pieces` colonnes), chacun perturbé par un hash déterministe de `(seed, index)` — pas un point uniformément aléatoire, pour éviter les esquilles dégénérées d'un Poisson pur. Chaque cellule est calculée par découpe successive du rectangle englobant contre le plan médiateur de chaque autre point (Sutherland-Hodgman, `O(pieces²)` — négligeable jusqu'à 64 pièces). Direction, magnitude du trajet, signe/magnitude du spin et valeur de profondeur par éclat sont tous des hashs de `(seed, index, salt)` distincts — deux rendus du même fichier au même instant sont donc octet pour octet identiques.

Pour chaque éclat, l'ordre des opérations canvas compte : **translation/rotation/échelle d'abord, découpe (`clip_path`) ensuite**, dans ce sens précis. Si la découpe est posée avant la transformation, le masque reste à sa position d'origine pendant que l'image sous-jacente se déplace dessous — l'éclat ne bouge jamais visuellement, seul son contenu glisse sous un trou fixe. C'est un bug qui a été observé et corrigé pendant l'implémentation ; un test dédié (`shatter_paints_ink_outside_the_nodes_own_box_where_an_intact_node_does_not`) l'aurait détecté en le repassant au rouge.

## Piège : `origin` est une fraction, pas des px

Contrairement à `TransformOrigin` (CSS, `LengthPercentage`), `shatter.origin` est une paire de flottants `0..1` relative à la boîte du nœud — `{ "x": 0.5, "y": 0.5 }` est le centre, `{ "x": 0.0, "y": 0.0 }` le coin haut-gauche. Donner des pixels ici ne produit pas d'erreur de schéma (le champ accepte n'importe quel flottant) mais un point d'origine hors de la boîte, donc une dispersion qui tire tous les éclats dans une direction quasi uniforme au lieu de rayonner.

## Cas dégénérés

- `pieces: 0` ou `1` est traité comme `1` (borné en interne) : toute la boîte forme un seul éclat, qui se contente de translater/tourner/rétrécir comme un bloc — pas d'erreur, juste un « shatter » dégénéré en simple sortie.
- `spread: 0` immobilise les éclats sur place : seuls le spin, la profondeur et le fondu restent visibles, une variante « dislocation sans envol ».
- `duration: 0` (ou négative) désactive l'effet à chaque frame, comme `chromatic_aberration`.
- `fade: false` laisse les éclats à pleine opacité même totalement dispersés — utile avec `mode: "hold"` pour une composition éclatée qui doit rester lisible.
