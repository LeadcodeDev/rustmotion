# `style.clip-path` — masquage non rectangulaire

`overflow: hidden` découpe au rectangle du parent. `clip-path` découpe l'élément
**lui-même** à une forme arbitraire — et contrairement à `overflow`, il emporte le
fond, la bordure et l'ombre externe avec lui, comme en CSS.

Sept formes, toutes résolues contre la **border box** du nœud :

| `kind` | Champs | Résolution des `%` |
|---|---|---|
| `none` | — | identique à l'absence de la propriété |
| `inset` | `top` `right` `bottom` `left`, `radius` optionnel | verticaux sur la hauteur, horizontaux sur la largeur |
| `circle` | `radius`, `origin` optionnel | `sqrt(w² + h²) / √2`, la référence CSS |
| `ellipse` | `rx` `ry`, `origin` optionnel | `rx` sur la largeur, `ry` sur la hauteur |
| `polygon` | `points`: `[[x, y], …]` | `x` sur la largeur, `y` sur la hauteur |
| `path` | `d`: données de chemin SVG | coordonnées relatives au coin haut-gauche de la boîte |
| `blob` | `radius`, `origin` optionnel, plus `lobes`/`wobble`/`seed` optionnels (mêmes défauts que `transition: { type: "blob" }` — `8`/`0.15`/`11`) | comme `circle` : `sqrt(w² + h²) / √2` |

`origin` prend la même forme que `transform-origin` et vaut le centre par défaut.

```json
{
  "type": "div",
  "style": {
    "width": 400, "height": 400, "background": "#8B5CF6",
    "clip-path": { "kind": "polygon",
                   "points": [[200, 0], [400, 400], [0, 400]] }
  }
}
```

Un cadre chanfreiné — les quatre coins coupés en diagonale — s'écrit en `polygon`
à huit points. C'est l'usage pour lequel la propriété a été câblée.

## Deux pièges

**`path` est en coordonnées locales.** Les données sont décalées du coin
haut-gauche de la boîte, pas du coin de la vidéo : un `M0 0` commence à l'angle de
l'élément. Un chemin copié depuis un éditeur SVG dont le viewBox ne commence pas à
l'origine arrivera décalé.

**`kind: node-path` n'est pas implémenté.** La variante existe au schéma — elle
désigne un autre nœud par `id` pour en reprendre la géométrie — mais rien ne la
résout encore : le pass de peinture n'a pas de table `id → chemin résolu`. Elle
n'échoue pas silencieusement, elle **écrit sur stderr** et ne découpe rien. Pour
un masque partagé entre deux nœuds, répète le même `kind: path` sur les deux en
attendant.

## Ce que `clip-path` ne fait pas

Il s'anime par `keyframes`, mais pas comme les autres propriétés : on déclare les
deux formes d'un coup avec `kind: "morph"` et c'est un scalaire,
`clip_path_progress`, qui balaie de l'une à l'autre. Voir
[clip-path-morph.md](clip-path-morph.md).

En revanche il ne s'interpole pas dans un `timeline`. C'est une propriété de peinture non
supportée à l'animation — voir [timeline-sequencing.md](timeline-sequencing.md).
Pour une révélation progressive, animer un `transform` sous un parent
`overflow: hidden` reste la voie.

## `blob` : la silhouette de la transition, en masque

`blob` fait grandir la même silhouette organique procédurale que la transition
[`blob`](mask-transition.md) — mêmes `lobes`, `wobble` et `seed`, **le même
générateur**, pas une seconde implémentation. La différence est ce qui la met à
l'échelle : `radius` autour de `origin`, au lieu du `progress` d'une transition.

Comme `path` et `node-path`, il n'est pas encore accepté par `kind: "morph"` —
seuls `inset`, `circle`, `ellipse` et `polygon` interpolent entre deux formes.
