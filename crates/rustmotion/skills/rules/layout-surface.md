# Poser une grille sur un cylindre ou une sphère : `style.layout-surface`

`transform` (`rotate-x`/`rotate-y`) plus `perspective` s'appliquent à un
nœud, mais ce nœud reste un plan. Donner sa propre rotation à chaque carte
d'un `for-each` ne le résout pas non plus : chaque carte a **son propre**
point de fuite, donc une grille inclinée reste un trapèze plat au lieu de se
courber — rien ne les fait converger vers un horizon commun. `layout-surface`
résout exactement ça : c'est une propriété du **conteneur**, pas de l'enfant.

```json
{ "type": "div", "style": {
    "display": "grid",
    "grid-template-columns": ["1fr","1fr","1fr","1fr","1fr","1fr","1fr","1fr","1fr"],
    "layout-surface": {
      "kind": "sphere", "radius": 1400, "arc_x": 120, "arc_y": 60,
      "perspective": 1600, "rotate_y": { "from": -15, "to": 15 }
    } },
  "children": [ "… neuf vignettes, disposées normalement par taffy …" ] }
```

## Ce que taffy voit, ce que la caméra voit

Le layout ne change pas : taffy calcule les colonnes/lignes exactement comme
sans `layout-surface`. La courbure n'existe qu'au **paint** — chaque enfant
direct du conteneur est repeint à sa position projetée sur le cylindre ou la
sphère, autour d'**un seul** point de fuite partagé par tout le conteneur.
C'est la même machinery que le tilt de caméra (#400, `apply_plane_camera` /
`css_perspective_m44`) appliquée par enfant plutôt que par plan entier — pas
une deuxième implémentation de la projection.

| Champ | Obligatoire | Rôle |
|---|---|---|
| `kind` | oui | `"cylinder"` (courbe seulement `arc_x`, la position verticale de chaque enfant n'est pas touchée) ou `"sphere"` (courbe `arc_x` **et** `arc_y`) |
| `radius` | oui | rayon en pixels |
| `arc_x` | non (0) | balayage angulaire total sur la largeur, en degrés |
| `arc_y` | non (0, `sphere` seulement) | balayage angulaire total sur la hauteur, en degrés |
| `perspective` | non | distance du point de fuite en pixels ; absent = courbe sans raccourcissement (orthographique) |
| `rotate_x` / `rotate_y` | non | bascule d'ensemble de la surface, en degrés ; fixe (`15`) ou animée (`{ "from": -15, "to": 15 }`, linéaire de `scene_duration` par défaut, ou `duration` explicite) |

Le centre de l'arc (`nu = nv = 0`, la cellule du milieu) reste à sa taille et
sa position normales — c'est la référence. Les cellules s'éloignent de plus
en plus du centre à mesure qu'elles approchent des bords de l'arc, ce qui
raccourcit leur largeur apparente sous la perspective : une grille projetée
n'a **pas** des cellules toutes de la même largeur, contrairement à une
grille plate.

## Piège : `cylinder` n'a pas de deuxième axe

`arc_y` n'existe que sur `sphere`. Un `cylinder` avec des rangées ne les
courbe pas verticalement — c'est voulu (un cylindre ne s'enroule que sur un
axe) : mettre `sphere` si les deux axes doivent se courber.

## Ordre de peinture : pas de tri en profondeur

Les enfants sont peints dans l'ordre de déclaration (`z-index`), comme un
conteneur plat — une cellule qui se courbe derrière une autre ne passe pas
automatiquement derrière elle au rendu. Pour un patch bien en-dessous de
180° (l'usage visé : un dôme de cartes, pas une sphère qui s'enroule
entièrement), chaque cellule fait toujours face à la caméra et ça ne se voit
pas. Un `arc_x`/`arc_y` très large, ou assez de `rotate_y` pour faire
franchir le limbe à une cellule, peut faire peindre une cellule lointaine
par-dessus une plus proche. Suivi général par #93, non traité ici.

## Ce qui n'y est pas

Pas de repli sur `taffy` pour re-calculer un flex/grid une fois courbé — le
flux reste celui d'un conteneur plat, seul le paint change. Un enfant qui a
lui-même un `transform` garde son comportement normal, appliqué *après* la
projection de la surface (les deux se composent).
