# Rule: `iris` — le masque qui grandit (ou se referme) depuis `origin`

`iris` est une `transition` : comme toute transition d'une vue `slide`, elle composite deux frame-buffers **déjà rendus**, ici au travers d'une forme qui grandit depuis `origin` (ou se referme dessus) et révèle la scène suivante. Voir la section « Composition » de `CLAUDE.md`.

```json
{
  "transition": {
    "type": "iris",
    "duration": 0.5,
    "easing": "ease_in_expo",
    "origin": { "x": 460, "y": 160 },
    "shape": "pill",
    "aspect": 2.2,
    "fill": "#FD2E92",
    "hold": 0.1,
    "ring": { "color": "#FFFFFF", "width": 6 },
    "reverse": false
  }
}
```

## Piège corrigé : `origin` était accepté et ignoré

Avant correction, `iris_transition` codait en dur `cx = w / 2, cy = h / 2` : `origin` passait la validation mais ne changeait jamais un seul pixel — le masque grandissait toujours depuis le centre du cadre. Le rayon maximal (celui qui doit atteindre le coin le plus éloigné) était aussi calculé pour un masque centré, donc systématiquement faux dès que `origin` n'est pas le centre. Les deux sont recalculés maintenant à partir du point réellement demandé : le rayon de couverture dépend de la distance de `origin` au coin le plus éloigné, pas de la diagonale du cadre divisée par deux.

`origin` prend la même forme que pour `zoom_blur` — `{ "x": …, "y": … }` en pixels du cadre, pas en `%`. Absent, il vaut le centre du cadre, comme avant.

## Champs

| Champ | Rôle | Défaut |
|---|---|---|
| `origin` | Centre du masque, en pixels du cadre. Absent = centre du cadre. | absent → centre |
| `shape` | `circle` (disque) ou `pill` (stade — rectangle aux bouts arrondis, étiré par `aspect`). | `circle` |
| `aspect` | `pill` uniquement : rapport largeur/hauteur pendant la croissance. `1.0` dégénère en cercle. Ignoré par `circle`. | `1.0` |
| `fill` | Couleur CSS peinte par le masque au lieu de révéler directement la scène suivante ; celle-ci apparaît en fondu une fois la couleur pleine trame atteinte. Absent = révélation directe. | absent |
| `hold` | Secondes de couleur pleine trame tenues avant le fondu vers la scène suivante. Sans effet si `fill` est absent. | `0` |
| `ring` | `{ "color": …, "width": … }` — un anneau coloré tracé sur le bord courant du masque. | absent |
| `reverse` | Le masque se referme sur `origin` au lieu de s'ouvrir depuis lui. | `false` |
| `duration`, `easing` | Communs à toutes les transitions. | `0.5`, `ease_in_out` |

## `fill` + `hold` : trois phases dans une seule transition

Sans `fill`, une seule phase : le masque grandit (ou se referme, avec `reverse`) sur toute la durée, la scène suivante apparaissant directement à l'intérieur.

Avec `fill`, la transition se découpe en trois segments successifs de `progress` :

1. **Croissance** — le masque de couleur pleine grandit sur la scène sortante, sur la première moitié du temps qui reste une fois `hold` retiré.
2. **Attente** — l'écran entier est la couleur `fill`, pendant `hold` secondes (converti en fraction de `duration`, plafonné à 90 % pour garder de la place à la croissance et à la révélation).
3. **Révélation** — fondu de la couleur pleine trame vers la scène entrante, sur la seconde moitié du temps restant.

`hold` est en **secondes**, comme `duration`, pas en fraction de `progress` — c'est pour ça que la transition a besoin de connaître sa propre `duration` pour convertir l'un en l'autre. Sans `fill`, `hold` est un champ ignoré (no-op), pas une erreur : même logique que `aberration` sur un `iris`, ou `cell` sur un `fade`.

## `reverse` : le masque se referme, il ne s'inverse pas seulement dans le temps

`reverse` échange à la fois **qui** occupe le masque et **le sens** du rayon : par défaut, la scène sortante (ou `fill`) remplit tout le cadre et le masque grandissant y ouvre une fenêtre vers la scène suivante ; avec `reverse`, c'est la scène sortante qui occupe le masque, et il **rétrécit** jusqu'à un point pendant que la scène suivante (ou `fill`) envahit tout l'espace libéré autour. Les deux lectures démarrent sur la frame sortante et finissent sur l'entrante — seule la géométrie intermédiaire change.

## `ring` : recalculé à chaque frame depuis `origin`/`shape`/`aspect`/le rayon courant

Le contournement précédemment nécessaire — un cercle tracé au trait, mis à l'échelle depuis le début de la scène suivante — ne pouvait qu'approximer le rayon réel de l'iris et prenait toujours du retard sur lui. `ring` trace le même chemin que le masque, à l'instant exact où il est peint : il suit donc le rayon (et la silhouette `pill` le cas échéant) sans dérive possible.

## Piège : `pill` grandit avec une marge de sécurité, pas une géométrie exacte

Un rectangle aux coins arrondis ne couvre pas ses coins exacts comme un disque couvre les siens — l'arrondi mange un peu de la diagonale. Plutôt que d'inverser cette géométrie précisément pour chaque `aspect`, le rayon maximal d'un `pill` est calculé avec une marge généreuse (~45 % au-delà du strict nécessaire) qui garantit la couverture pour des `aspect` raisonnables (le rapport largeur/hauteur du cadre lui-même borne combien `origin` peut être excentré). Ce n'est pas un souci en pratique — la transition est rapide et la marge invisible à l'écran — mais ça veut dire que la taille exacte du masque à un instant donné n'est pas une formule à inverser pour caler un autre élément dessus ; utiliser `ring` pour ça, justement.
