# Rule: `shape.draw_start` et `shape.path_morph` — trim des deux bouts, et `d` animé

Deux champs propres au composant `shape`, pas des propriétés `style.animation` génériques comme `draw_progress` — la nuance compte, voir plus bas.

## `draw_start` : l'autre bout du trait

`draw_progress` anime la **fin** du trait visible, toujours depuis le début du chemin. `draw_start` anime son **début** — le segment visible est le chemin entre `draw_start` et `draw_progress`, tous deux en fraction `0..1` de la longueur du chemin.

```json
{
  "type": "shape",
  "shape": { "path": { "data": "M0 80 C 20 40, 40 40, 50 10 C 60 40, 80 40, 90 80" } },
  "stroke": { "color": "#F68F2B", "width": 6 },
  "draw_start": 0.5,
  "style": {
    "animation": [{
      "name": "keyframes",
      "keyframes": [
        { "property": "draw_progress", "keyframes": [{ "time": 0, "value": 0 }, { "time": 0.5, "value": 1 }] }
      ]
    }]
  }
}
```

`draw_start` accepte un nombre littéral ou une expression `"= …"` (même grammaire que `stroke.dash_offset`), réévaluée chaque frame contre `$t`/`$t_abs`/`$duration`/`$width`/`$height`/`$fps`. Pour un segment qui « marche » le long du trait :

```json
"draw_start": "= max(0, ($t - 0.9) / 0.5)"
```

### Ce n'est pas une propriété `keyframes` comme `draw_progress`

`draw_progress` vit dans `AnimatedProperties`, résolu par le pipeline générique `animator.rs`/`style.animation` — la même mécanique que `opacity`, `translate_x`, etc. `draw_start` **n'y vit pas** : c'est un champ propre à `Shape`, résolu localement via `crate::expr::Computed<f32>`, exactement comme `stroke.dash_offset`. Une tentative de le piloter par un `keyframes` de `style.animation` avec `"property": "draw_start"` ne fait rien — ce nom n'existe pas dans `AnimatedProperties`. Deux raisons à ce choix :

1. **Ownership** : `AnimatedProperties`/`animator.rs` appartenaient à un autre chantier au moment où `draw_start` a été ajouté ; y ajouter un champ (et ses points de dispatch : `merge`, lecture, écriture) n'était pas dans le périmètre de ce changement.
2. `Computed<f32>` couvre le même besoin sans dupliquer la mécanique de keyframes/easing d'`animator.rs` — au prix de perdre l'easing nommé (`ease_in_out_cubic`, etc.) au profit d'une expression écrite à la main.

**Limite connue** : `svg`, `line`, `arrow` et `connector` n'ont pas cette capacité — seul `shape` l'a. Étendre `draw_start` à ces composants demande d'ajouter le champ à `AnimatedProperties` (avec son sentinel `-1.0`, son `merge`, ses deux points de dispatch par nom de propriété) et de le brancher dans chacun de ces fichiers.

### Sans `draw_progress` actif, `draw_start` grignote depuis la fin implicite de `1.0`

Si `draw_progress` n'anime jamais (reste au sentinel `-1.0`, c'est-à-dire absent), le trait est normalement dessiné en entier. `draw_start` s'applique quand même dans ce cas : la fin effective vaut `1.0`, donc `draw_start` seul efface progressivement la queue du trait sans qu'il ait besoin de croître depuis `0` au préalable.

### Zéro à l'origine, comme `line`/`svg`

Un `draw_start >= draw_progress` (fenêtre vide ou inversée) ne peint rien — la trimming passe par `trim_path_between`, une extraction géométrique réelle via `PathMeasure::get_segment`, pas un `PathEffect::dash` à intervalle nul. C'est précisément le piège documenté dans [rules/draw-progress-stroke.md](draw-progress-stroke.md) (un pointillé de longueur nulle avec un cap arrondi peint quand même un point) : `trim_path_between` ne produit jamais un tel pointillé, un chemin vide n'a aucun verbe à peindre.

## `path_morph` : `d` animé, keyframe par keyframe

```json
{
  "type": "shape",
  "shape": { "path": { "data": "M10 80 C 20 40, 40 40, 50 10 C 60 40, 80 40, 90 80 Z" } },
  "fill": "#8B5CF6",
  "path_morph": {
    "keyframes": [
      { "time": 0, "value": "M10 80 C 20 40, 40 40, 50 10 C 60 40, 80 40, 90 80 Z" },
      { "time": 0.6, "value": "M10 80 C 25 45, 35 35, 50 15 C 65 35, 75 45, 90 80 Z" }
    ],
    "easing": "ease_in_out",
    "repeat": true,
    "yoyo": true
  }
}
```

`path_morph`, présent, **remplace entièrement** `shape` pour le rendu — fond et trait — tant qu'il est défini ; `shape.shape` sert seulement de secours si `path_morph` échoue à produire un chemin. `time` est en secondes depuis le début de la scène, comme partout ailleurs dans ce schéma — pas une fraction `0..1`. `repeat` boucle au premier keyframe une fois le dernier `time` dépassé ; `yoyo` (avec `repeat`) alterne le sens au lieu de revenir sèchement au début.

### Structure incompatible : loud, pas silencieux

L'interpolation ne marche que point par point, entre deux chemins qui partagent exactement la même suite de commandes (même verbes, dans le même ordre, avec le même nombre de points). Un décalage — un `Q` remplacé par un `L` à la même position, un sommet en plus — **écrit sur stderr** et retient la forme du keyframe précédent plutôt que d'interpoler n'importe quoi ou de planter, le même principe que `clip_path_to_skia` pour `kind: node-path` (voir [rules/clip-path.md](clip-path.md)) : rendre visible plutôt que deviner. Il n'y a pas de ré-échantillonnage automatique vers un nombre commun de segments cubiques — construire les deux `d` avec la même structure de commandes reste la responsabilité de l'auteur.

### Ce qui est réutilisé

`trim_path_between` et `interpolate_path_data` vivent dans `rustmotion_core::engine::renderer` (le même module que `build_shape_path`/`draw_shape_path`), pas seulement pour `shape` : ce sont des primitives Skia pures, sans rien de propre au composant, prêtes à être consommées par `svg`/`line`/`arrow`/`connector` le jour où `draw_start` leur est étendu.
