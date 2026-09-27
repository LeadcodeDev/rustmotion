# Animer `clip-path` : `kind: "morph"`

[clip-path.md](clip-path.md) décrit les six formes statiques. Aucune ne
s'anime — un `clip-path` posé en `style` est figé pour toute la scène. `morph`
(issue #384) est la septième forme, et la seule pensée pour bouger : elle fixe
deux formes (`from`/`to`) et une `progress` qui balaie l'une vers l'autre.

```json
{
  "type": "div",
  "style": {
    "width": 1920, "height": 1080,
    "clip-path": {
      "kind": "morph",
      "from": { "kind": "polygon", "points": [[320,0],[1600,0],[1920,320],[1920,1080],[0,1080],[0,320]] },
      "to":   { "kind": "polygon", "points": [[0,0],[1920,0],[1920,780],[1620,1080],[300,1080],[0,780]] },
      "progress": 0
    },
    "animation": [
      { "name": "keyframes", "keyframes": [
        { "property": "clip_path_progress", "easing": "ease_in_out",
          "keyframes": [ { "time": 0, "value": 0 }, { "time": 2, "value": 1 } ] }
      ] }
    ]
  }
}
```

`progress` littéral dans `clip-path` n'est que la position au repos ; c'est
l'animation `keyframes` sur la propriété `clip_path_progress` (une propriété
scalaire ordinaire, au même titre que `opacity` ou `border_radius`) qui la
fait bouger — `spring`, `loop`, easing par keyframe : tout ce que `keyframes`
sait déjà faire sur un scalaire s'applique donc ici aussi.

## Ce qui s'interpole

| `from`/`to` | Interpolation |
|---|---|
| `polygon` / `polygon`, **même nombre de points** | point par point |
| `inset` / `inset` | `top`/`right`/`bottom`/`left`/`radius` un par un |
| `circle` / `circle` | `radius` et `origin` |
| `ellipse` / `ellipse` | `rx`/`ry` et `origin` |

`from` et `to` doivent être **le même `kind`**, et pour `polygon` **le même
nombre de points** — sans ça il n'existe aucune correspondance point-à-point à
interpoler. Le mismatch n'est pas absorbé silencieusement : un message est
écrit sur stderr nommant les deux `kind` (ou les deux comptes de points), et le
nœud reste **non clippé** pour la frame — pas de snap brutal entre les deux
formes.

`none`, `path` et `node-path` ne sont pas interpolables (pas de correspondance
géométrique évidente) ; les utiliser comme `from` ou `to` tombe dans le même
cas « non clippé + message stderr ».

## Ce que `morph` ne fait pas

- Un seul couple `from`/`to` par nœud — pas de chaîne à N formes sur une seule
  propriété. Pour plusieurs silhouettes successives, il faut plusieurs beats
  (scènes ou nœuds superposés avec un crossfade), pas un seul `morph`.
- `timeline` + `style.transition` ne lisse toujours pas `clip-path` (voir
  [timeline-sequencing.md](timeline-sequencing.md)) : un `clip-path` posé par
  un pas de `timeline` continue de sauter à l'instant du pas, `morph` ou pas.
  `morph` ne s'adresse qu'au cas `keyframes`.
