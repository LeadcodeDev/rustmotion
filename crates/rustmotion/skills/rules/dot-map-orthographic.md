# `dot_map` en globe : `projection: "orthographic"`

Par défaut `dot_map` est une carte plate (équirectangulaire — `lng`/`lat`
mis à l'échelle linéairement sur la box). Un `clip-path: circle` dessus ne
fait que découper un disque dans cette carte plate : les points restent
alignés sur une grille linéaire, ils ne convergent jamais vers le bord. Pour
un vrai globe vu de l'extérieur, il faut `projection: "orthographic"`.

```json
{ "type": "dot_map", "projection": "orthographic",
  "rotate": { "lng": { "from": -20, "to": 40 }, "lat": 15 },
  "limb_shading": 0.6,
  "points": [{ "lat": 48.85, "lng": 2.35, "label": "Paris" }],
  "arcs": [{ "from": [48.85, 2.35], "to": [40.71, -74.0], "draw_in": 0.8 }] }
```

## Ce que fait la projection

`orthographic_project(lat, lng, lat_centre, lng_centre)` est la formule
fermée standard (pas une approximation) : elle renvoie une position normée
sur la sphère unité plus `cos_c`, le cosinus de la distance angulaire au
point sous l'observateur (`1.0` au centre du disque, `0.0` au limbe,
négatif sur l'hémisphère caché). Un point à `cos_c < 0` n'est pas dessiné —
c'est la culling de l'hémisphère lointain que le `clip-path` ne peut pas
faire, faute de savoir qu'il y a une sphère dessous.

| Champ | Défaut | Rôle |
|---|---|---|
| `projection` | `"equirectangular"` | `"orthographic"` active tout ce qui suit ; sinon ils sont inertes |
| `rotate.lng` | `0` | méridien face caméra ; fixe ou `{ "from", "to", "duration"? }` (linéaire depuis `start_at`, `duration` par défaut = `animation_duration`) — la rotation autour du pôle |
| `rotate.lat` | `0` | parallèle face caméra ; fixe uniquement |
| `limb_shading` | absent | assombrit les points vers le bord, `0` (rien) à `1` (noir au tout bord) |
| `arcs[].draw_in` | `1.0` | fraction de l'arc tracée depuis `from`, comme `draw_progress` sur `line`/`arrow` mais figée (pas pilotée par le temps) |
| `arcs[].altitude` | `0.12` | hauteur du décollement au sommet de l'arc, en fraction du rayon |

Le rayon du disque est calculé depuis la box (`min(largeur, hauteur) * 0.94
/ 2`), centré. `dot_spacing`/`dot_radius` gardent leur sens : l'espacement
en pixels est converti en pas angulaire au rayon du disque, ce qui fait que
les points sont semés à angle constant sur la sphère — leur espacement à
l'écran, lui, **rétrécit vers le limbe**, exactement l'effet recherché.

## Les arcs suivent un grand cercle, pas une ligne droite à l'écran

Chaque arc est interpolé en 3D (slerp entre les deux vecteurs unitaires
`from`/`to`), pas en lat/lng — une interpolation linéaire en lat/lng ne suit
pas un grand cercle. Chaque échantillon repasse par
`orthographic_project` : un arc qui traverse l'horizon est coupé en
segments visibles, jamais tracé tout droit à travers le globe. Un arc
entièrement sur l'hémisphère caché ne peint rien du tout.

## Piège : `rotate`/`limb_shading`/`arcs` sont inertes en mode plat

Les déclarer sans `projection: "orthographic"` ne fait rien — la carte
plate historique reste strictement identique à avant #387, à l'octet près
(c'est testé). Le mode plat n'a pas de notion d'hémisphère caché ni de
limbe : ces champs n'ont de sens que pour un globe.

## Ce qui n'y est pas

Pas de tampon de profondeur entre les points, les arcs et les dots du fond :
ils sont peints dans l'ordre (fond, puis points, puis arcs), qui suffit tant
que les arcs restent au-dessus du globe — comme `layout-surface`, aucun tri
par profondeur véritable n'est fait. Voir
[layout-surface.md](layout-surface.md) pour l'autre moitié de #387, qui
partage la même limite et la même justification (#93).
