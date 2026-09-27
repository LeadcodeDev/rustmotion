# Rule: `feather` et `band_color` — un bord doux, éventuellement teinté

`feather` et `band_color` sont deux champs partagés par les `wipe_*` (`wipe_left`, `wipe_right`, `wipe_up`, `wipe_down`), `mask` et `blob` (voir [rules/mask-transition.md](mask-transition.md)). Ignorés par toute autre transition.

```json
{
  "transition": {
    "type": "wipe_left",
    "duration": 0.5,
    "feather": 320,
    "band_color": "#FF8CC6"
  }
}
```

## `feather` : la largeur du dégradé, en px

`0` (le défaut) est un bord dur — exactement le comportement d'avant ce champ, pas d'approximation, pas de passe de flou du tout. Une valeur positive adoucit la limite mobile (le bord du wipe, ou le contour de la silhouette pour `mask`/`blob`) sur cette largeur en pixels du cadre final, via un `MaskFilter::blur` gaussien appliqué à un masque alpha peint puis lu en niveaux de gris — pas un dégradé linéaire à la main, ce qui suit naturellement la courbure d'une silhouette `mask`/`blob` aussi bien que le bord droit d'un wipe.

## `band_color` : un front coloré qui balaie le bord

Sans `band_color`, le dégradé mélange simplement les deux scènes. Avec, une teinte s'ajoute **au pic exact du bord**, en s'estompant vers les deux côtés de la bande de `feather` — un front coloré qui traverse l'écran avant que la scène entrante ne soit pleinement visible. Le poids de la teinte suit `4·a·(1-a)` où `a` est l'alpha du masque flouté : nul quand `a` vaut `0` ou `1` (loin du bord), maximal pile à `a = 0.5` (le bord lui-même) — une parabole, pas une bande à largeur fixe à régler séparément de `feather`.

`band_color` est un **no-op sans `feather`** : un bord dur n'a pas de bande à teinter. Le mettre avec `feather: 0` ne change rien à l'image.

## Zéro aux deux bouts, même avec un `feather` large

Toutes les transitions qui acceptent `feather` commencent par le même court-circuit que `mask`/`blob`/`zoom_blur`/`whip` : `progress <= 0.0` rend la frame sortante **brute**, `progress >= 1.0` la frame entrante **brute**, avant toute passe de flou. Sans ce court-circuit, un `feather` large ferait déborder la bande floutée au-delà du bord du cadre au tout début ou à la toute fin de la transition, laissant fuir un peu de la scène adjacente — précisément le défaut que l'issue d'origine proscrit pour `zoom_blur`/`whip`, et qui s'appliquerait tout autant ici sans le même court-circuit.

## Comment c'est construit

`feather <= 0.0` (et pas de `band_color`) garde le chemin rapide historique : un simple `clip_rect`/`clip_path` sur les images déjà peintes, sans passe de flou. Dès que l'un des deux est actif, le bord (rectangle de révélation pour un wipe, silhouette mise à l'échelle pour `mask`/`blob`) est peint dans une surface `Alpha8` avec un `MaskFilter::blur`, relu comme un masque de mélange pixel par pixel entre les deux frames — le même mécanisme, `composite_through_mask`, sert les trois familles de transitions.

## Piège : un `feather` très supérieur aux dimensions du cadre delaie visuellement le bord dur

Le sigma du flou gaussien est dérivé de `feather` (`feather / 3`, à peu près la largeur perçue du dégradé) — un `feather` de plusieurs milliers de pixels sur un cadre HD produit un dégradé qui occupe tout l'écran en permanence, jamais un vrai bord net. Ce n'est pas un bug : `feather` est une largeur de bord, pas un rayon de flou d'ambiance — pour un fondu pleine trame, `fade` reste le bon outil.
