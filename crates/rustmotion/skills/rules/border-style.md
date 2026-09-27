# `border.style` — dashed, dotted, double

`BorderStyle` accepte `none | solid | dashed | dotted | double`. Les cinq peignent
désormais un rendu distinct (issue #375) — avant, tout sauf `none` produisait le
même anneau plein que `solid`.

| `style` | Rendu |
|---|---|
| `none` | rien |
| `solid` | anneau plein (comportement historique, inchangé) |
| `dashed` | tirets de longueur `3×width`, espacés de `3×width` |
| `dotted` | points ronds de diamètre `width`, espacés d'environ `2×width` |
| `double` | deux traits de `width/3`, séparés par un espace de `width/3` |

```json
{
  "type": "div",
  "style": {
    "width": 300, "height": 200, "border-radius": 20,
    "border": { "color": "#FD2E92", "width": 4, "style": "dashed" }
  }
}
```

Le trait suit le contour arrondi (`border-radius` s'applique normalement) :
`dashed`/`dotted` tracent le tracé au **centre** de l'épaisseur de bordure
(comme la version pleine), `double` place un trait au bord externe et un au
bord interne, avec l'écart au milieu.

**Limite connue :** les quatre côtés partagent une seule épaisseur de trait —
celle du côté le plus large (`border.width` par côté n'est pas pris en compte
séparément pour choisir la cadence des tirets/points). Un besoin de bordure
dashed asymétrique (haut ≠ droite) n'est pas couvert.

## Animer `border-radius` en `keyframes`

`border_radius` est une propriété scalaire acceptée par l'animation
`keyframes` — un carré qui devient un cercle, ou l'inverse (issue #373) :

```json
{
  "type": "div",
  "style": {
    "width": 200, "height": 200, "background": "#000000",
    "animation": [
      { "name": "keyframes", "keyframes": [
        { "property": "border_radius", "keyframes": [
          { "time": 0, "value": 0 }, { "time": 1, "value": 100 }
        ] }
      ] }
    ]
  }
}
```

Elle interpole toujours vers un rayon **uniforme** (un seul nombre en px) —
comme pour `style.transition`/`timeline` (voir
[timeline-sequencing.md](timeline-sequencing.md)), les rayons par coin
(`{ "top-left": ..., ... }`) ne sont pas animables.
