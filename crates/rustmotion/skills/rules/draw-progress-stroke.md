# `draw_progress` : rien à 0, et un trait qui ne change pas d'apparence en finissant

`draw_progress` révèle un trait progressivement. On le pilote par un preset
`draw_in`/`stroke_reveal`, ou par des `keyframes` sur la propriété du même nom.
Trois pièges à connaître sur `line` et `svg` (`reveal: "stroke"`, celui par
défaut).

## `svg` : `draw: true` n'est pas un pilote

`draw: true` force le **chemin de rendu** « draw-on ». Il ne fait pas avancer
`draw_progress`. Sans pilote, la propriété reste à sa valeur au repos, le
peintre prend la branche « fini » (`progress >= 1.0`, qui délègue simplement à
resvg) et la marque se rend **exactement comme avec `draw: false`** — vérifié
octet pour octet sur deux PNG.

Le validateur refuse donc `draw: true` sans pilote, plutôt que de laisser le
drapeau avoir l'air de faire quelque chose :

```
draw: true but nothing animates draw_progress — the mark renders finished,
pixel-identical to draw: false. Add a 'draw_in' or 'stroke_reveal' preset,
or keyframes on 'draw_progress'.
```

En pratique on n'a d'ailleurs pas besoin de `draw: true` : un preset `draw_in`
suffit à lui seul, puisque le peintre bascule dès que `draw_progress` est dans
`[0, 1[`.

## `line` : `draw_progress: 0` ne doit rien peindre

`Line::paint` force un cap arrondi (`PaintCap::Round`) et construit un
pointillé `[longueur_dessinée, reste]` pour révéler le trait. À
`draw_progress: 0`, `longueur_dessinée` vaut `0` — un pointillé de longueur
nulle avec un cap arrondi se peint quand même : Skia dessine un point plein
d'un diamètre égal à `width`, exactement au point de départ. Le composant
retourne maintenant sans rien peindre dès que `draw_progress <= 0` (dans la
fenêtre `[0, 1[` — `draw_progress` absent ou `>= 1` reste le trait complet,
inchangé).

## `svg` en train de se dessiner doit ressembler au trait fini

Pendant le tracé (`paint_draw_on`, `draw_progress` dans `]0, 1[`), le trait
doit avoir la **même** épaisseur, le même `stroke-linecap` et le même
`stroke-linejoin` que le rendu final (`progress >= 1`, peint par `resvg`) —
sinon la dernière frame du tracé et la première frame « finie » ne se
raccordent pas visuellement (saut d'épaisseur, apparition brusque d'un cap).

Concrètement :

- Le canevas est déjà mis à l'échelle du `viewBox` vers la taille du nœud
  (`canvas.scale((scale_x, scale_y))`) avant de peindre chaque segment : le
  `stroke-width` du SVG source doit être posé tel quel sur le `Paint`, sans
  compensation supplémentaire. Diviser par le facteur d'échelle annule cette
  mise à l'échelle et fige le trait à sa largeur SVG brute, quelle que soit
  la taille du nœud — le bug qu'un remaniement futur ne doit pas
  réintroduire.
- `stroke-linecap`/`stroke-linejoin` du `<path>` source (lus sur
  `usvg::Stroke`) doivent être posés sur le `Paint` de chaque segment, pas
  seulement utilisés pour le rendu final. Un cap `round` sur le trait fini
  mais `butt` (le défaut de Skia) pendant le tracé fait apparaître le cap
  d'un coup à `draw_progress = 1`, avec une extension visible du trait
  (le rayon du cap).

`marquee` et `cursor` restent hors sujet ici : ce ne sont pas des traits
révélés par `draw_progress`.
