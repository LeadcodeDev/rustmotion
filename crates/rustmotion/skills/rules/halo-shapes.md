# `halo` — zones elliptiques (`radius_x`/`radius_y`/`rotation`)

Jusqu'ici, une zone `halo` n'avait qu'un `radius` : un cercle, point. Trois
champs supplémentaires sur chaque zone permettent un ovale — utile pour un
filet de lumière fin et large en haut de cadre, une ambiance beaucoup plus
proche d'une vraie key light que le blob rond par défaut.

| Champ | Type | Défaut | Rôle |
|---|---|---|---|
| `radius` | `f32` | `0.4` | Rayon du cercle, en fraction de `max(largeur, hauteur)` de la surface (viewport en vue `slide`, monde en vue `world`). Inchangé. |
| `radius_x` | `f32?` | absent → retombe sur `radius` | Rayon horizontal, mêmes unités que `radius`. |
| `radius_y` | `f32?` | absent → retombe sur `radius` | Rayon vertical, mêmes unités que `radius`. |
| `rotation` | `f32` | `0.0` | Rotation de l'ellipse en degrés, sens horaire, autour de son propre centre. |

Tous en **snake_case** dans le JSON (`radius_x`, pas `radius-x`) — contrairement
au kebab-case de `animated-background` ou `world-position` au niveau scène.
`HaloZone` est un objet imbriqué (`zones: [...]`) et suit la casse de ses
voisins directs (`radius`, `opacity`), pas celle du schéma racine.

## Compatibilité : un cercle reste un cercle, au bit près

Omettre `radius_x`/`radius_y` retombe sur `radius` pour les deux axes — une
zone écrite avant l'existence de ces champs continue à produire exactement
les mêmes pixels. Ce n'est pas une promesse de "même rendu visuel" : le
moteur détecte qu'une zone est circulaire (`radius_x == radius_y` une fois
les valeurs par défaut appliquées) et prend alors le même chemin de code que
l'ancien `draw_circle`, sans jamais passer par l'ellipse ni par la rotation.
Fixer explicitement `radius_x`/`radius_y` à la même valeur que `radius`
produit donc le rendu identique à ne rien fixer du tout — c'est la même
branche qui s'exécute.

**Corollaire :** `rotation` sur une zone circulaire est un pur no-op, pas
seulement "sans effet visuel" — le champ n'est même pas lu. Un cercle tourné
est un cercle ; ça n'aurait forcé qu'un calcul de matrice de rotation pour
rien, avec le risque de décaler l'anti-aliasing au bord d'un fragment de
pixel entre deux exécutions. `rotation` ne prend effet que si l'ellipse est
réellement ovale (`radius_x != radius_y`).

## Recette : filet de lumière large et fin en haut de cadre

```json
{
  "preset": "halo",
  "zones": [
    {
      "color": "#8B5CF6AA",
      "x": 0.5,
      "y": 0.02,
      "radius_x": 0.85,
      "radius_y": 0.07,
      "rotation": 0
    }
  ]
}
```

`x`/`y` restent le centre de l'ellipse (pas un coin) : `y: 0.02` place ce
centre presque au bord haut, et comme `radius_y` est petit, la moitié basse
de l'ellipse qui déborderait sous le cadre ne se voit simplement pas — pas
besoin de la sortir du viewport à la main. Une inclinaison légère se fait
avec `"rotation": -8` : le filet suit alors une diagonale au lieu d'être
parfaitement à plat.

## Flou : calé sur l'axe le plus fin, pas sur le plus large

Le flou gaussien de la zone est proportionnel au **plus petit** des deux
rayons effectifs (`min(radius_x, radius_y) * 0.15`), pas à leur moyenne ni au
plus grand. Un ovale large de `radius_x: 0.85` et fin de `radius_y: 0.07`
garde un bord net à l'échelle de son épaisseur réelle ; caler le flou sur
`radius_x` aurait noyé tout le filet dans un flou disproportionné par rapport
à sa hauteur.

## Respiration (`breath`) : les deux axes bougent ensemble

L'animation de respiration existante (le halo qui pulse doucement, pilotée
par `speed` sur le fond animé) multiplie `radius_x` et `radius_y` par le
**même** facteur à chaque frame — l'ellipse pulse en conservant son rapport
d'aspect, elle ne devient jamais plus ronde ou plus écrasée en respirant.

## Ça marche aussi dans `view.background` (vue `world`)

Rien de spécifique à `radius_x`/`radius_y`/`rotation` par rapport au reste de
`HaloZone` : la même zone posée en `view.background` d'une composition
`world` (voir [world-view.md](world-view.md) §4) hérite du même comportement,
`x`/`y`/`radius*` restant des fractions du monde plutôt que du viewport.

## Transition entre deux fonds `halo`

L'interpolation utilisée pour un `background.transition` entre deux scènes
`halo` lisse maintenant `radius_x`, `radius_y` et `rotation` au même titre que
la couleur ou la position — plus de saut brutal de forme à la coupe si la
scène d'arrivée a une ellipse différente (ou une rotation différente) de la
scène de départ.
