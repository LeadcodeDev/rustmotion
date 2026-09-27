# Rule: `mask` et `blob` — `iris` généralisé à une silhouette arbitraire

`mask` et `blob` sont des `transition` (au même titre que `iris`, `slide`, `chromatic_wipe`…) : comme toute transition d'une vue `slide`, elles compositent deux frame-buffers **déjà rendus** — aucun élément ne survit à la coupe, seuls les pixels sont mélangés. Voir la section « Composition » de `CLAUDE.md`.

`iris` grandit un cercle (ou un stade) depuis `origin`. `mask` fait la même chose avec **n'importe quelle silhouette** — un logo, un blob organique, une forme chanfreinée — et `blob` grandit une silhouette organique procédurale sans avoir à en fournir les points.

## `mask` : une silhouette au choix

```json
{
  "transition": {
    "type": "mask",
    "duration": 0.6,
    "easing": "ease_in_cubic",
    "silhouette": {
      "kind": "path",
      "d": "M50 0 L61 35 L98 35 L68 57 L79 91 L50 70 L21 91 L32 57 L2 35 L39 35 Z"
    },
    "origin": { "x": 960, "y": 540 },
    "from_scale": 0.05,
    "to_scale": 14,
    "feather": 2
  }
}
```

`silhouette` reprend le même vocabulaire `kind` que `style.clip-path` (voir [rules/clip-path.md](clip-path.md)) : `polygon` (`points: [[x, y], …]`) ou `path` (`d`, données SVG). Les deux sont exprimés dans les unités **propres à la silhouette** — celles de l'exemple ci-dessus décrivent une étoile dans une boîte 0-100, mais rien n'impose cette échelle : un `path` copié depuis une icône 24×24 fonctionne pareil.

## Champs

| Champ | Rôle | Défaut |
|---|---|---|
| `silhouette` | `mask` uniquement : la forme, `polygon` ou `path`. Requis pour `mask` — absente ou dégénérée (moins de 3 points, `d` non parsable), la transition retombe sur un `fade` et l'écrit sur stderr plutôt que de planter. | absent |
| `origin` | `mask`/`blob` : le point (pixels du cadre, **pas** une fraction `0..1`) autour duquel la silhouette grandit — le centre de sa propre boîte englobante vient s'y caler. Même convention que `zoom_blur`/`iris`. | centre du cadre |
| `from_scale` | `mask` uniquement : le facteur d'échelle à `progress: 0`. Une petite valeur positive (le défaut) laisse un point à peine visible, sans conséquence puisque `progress <= 0.0` retourne toujours la frame sortante brute, court-circuit compris. | `0.0` |
| `to_scale` | `mask` uniquement : le facteur d'échelle à `progress: 1`. **À la charge de l'auteur** — contrairement à `iris`, le rayon de couverture d'une silhouette arbitraire ne se résout pas automatiquement. Trop petit laisse un résidu de l'ancienne scène dans les coins. | `20.0` |
| `lobes`, `wobble`, `seed` | `blob` uniquement : nombre de lobes, amplitude du tremblement (fraction du rayon, `0` = cercle), et graine stable — même sélecteur que `pixel_dissolve.seed`. | `8`, `0.15`, `11` |
| `feather`, `band_color` | Communs à `mask`/`blob` et aux `wipe_*` — voir [rules/feathered-wipes.md](feathered-wipes.md). | `0`, absent |
| `duration`, `easing` | Communs à toutes les transitions. | `0.5`, `ease_in_out` |

## `blob` : la même croissance, sans fournir la silhouette

```json
{
  "transition": {
    "type": "blob",
    "duration": 0.6,
    "lobes": 9,
    "wobble": 0.15,
    "seed": 3,
    "origin": { "x": 960, "y": 320 }
  }
}
```

`blob` construit `lobes` points autour d'un cercle, chacun décalé radialement par un bruit stable dérivé de `seed` (`wobble` en fixe l'amplitude), puis relie les points par des courbes quadratiques passant par leurs milieux — un contour lisse, sans coin dur. Contrairement à `mask`, le rayon de couverture est **résolu automatiquement**, comme celui d'`iris` : `wobble` fait que certains lobes sont plus courts que le rayon nominal, donc le rayon cible est divisé par `(1 - wobble)` pour garantir qu'même le lobe le plus rétréci atteigne le coin le plus éloigné de `origin` à `progress: 1`.

`seed` (et `lobes`/`wobble`) fixent entièrement la forme : deux transitions avec les mêmes valeurs produisent des octets identiques.

## Zéro aux deux bouts, par construction — pas par réglage fin

Comme `zoom_blur` et `whip`, `mask_transition` et `blob_transition` commencent par un court-circuit : `progress <= 0.0` retourne la frame sortante **brute**, `progress >= 1.0` la frame entrante **brute**, avant tout calcul de silhouette ou de rayon. Pour `mask`, ça veut dire qu'un `from_scale` non nul (le défaut) ne laisse jamais un résidu visible au tout premier instant — le court-circuit l'emporte toujours sur la géométrie. Pour `blob`, la marge de sécurité sur le rayon (`/(1 - wobble)`) garantit la couverture bien avant `progress: 1`, et le court-circuit garantit l'exactitude pile à la borne.

## Piège : `to_scale` trop petit sur `mask`

Contrairement à `iris` (cercle ou stade, dont le rayon de couverture se calcule exactement depuis `origin` et les dimensions du cadre), une silhouette arbitraire n'a pas de formule générale pour « le facteur qui couvre tout le cadre ». Si `to_scale` est trop petit, `progress: 1` bascule quand même sur la frame entrante intacte (le court-circuit ne dépend pas de `to_scale`) — mais l'instant juste avant laisse un cadre visible de l'ancienne scène dans les coins que la silhouette n'a pas atteints. Ce n'est pas un bug : ajuster `to_scale` à la géométrie de la silhouette et du cadre est le prix de la généralité par rapport à `iris`.

## Ce qui est réutilisé, ce qui ne l'est pas

`mask_transition`/`blob_transition` réutilisent directement `iris_max_radius` (pour `blob`, en `IrisShape::Circle`) et le même schéma `origin` que `zoom_blur`/`iris`. `silhouette` reprend le vocabulaire `kind: polygon | path` de `style.clip-path`, y compris l'appel à `skia_safe::Path::from_svg` pour `path` — mais pas le type `ClipPath` lui-même, ni `clip_path_to_skia` (qui résout des `%` contre une *layout box*, une notion qui n'existe pas pour une transition compositant deux buffers déjà peints). La mise à l'échelle autour du centre de la boîte englobante de la silhouette, et le compositing feather/`band_color`, sont propres à cette transition.
