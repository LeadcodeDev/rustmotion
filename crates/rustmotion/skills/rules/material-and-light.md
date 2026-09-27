# `style.material` et `scene.light` : des surfaces éclairées

Ce qui fait lire une tuile comme « rendue en 3D » n'est pas le reflet pris
isolément, c'est que **plusieurs formes soient éclairées pareil**. D'où deux
pièces et non une : le nœud déclare sa matière, la scène déclare d'où vient la
lumière.

```json
"scenes": [{
  "light": { "x": -0.4, "y": -0.85, "intensity": 1.0 },
  "children": [
    { "type": "div", "style": { "background": "#6D28D9", "border-radius": 28,
                                "material": "glossy" } }
  ]
}]
```

`light` est **optionnel** : sans lui, la lumière vient du haut-gauche, là où l'œil
l'attend sans qu'on le lui dise. Un scénario peut donc n'écrire que `material`.

## Les trois presets

| Preset | Reflet | Arête spéculaire | Ombre |
|---|---|---|---|
| `glossy` | large et vif | oui, marquée | douce, opposée |
| `metal` | resserré, plus sourd | oui, plus dure | plus marquée |
| `matte` | aucun | aucune | seule couche peinte |

`matte` existe pour poser une surface plate à côté de surfaces brillantes sans
qu'elle ait l'air non éclairée : elle prend la lumière, elle n'en renvoie pas.

Forme longue pour doser : `"material": { "preset": "glossy", "intensity": 0.6 }`.
L'intensité du nœud est **multipliée** par celle de la scène, donc
`light.intensity: 0` aplatit toute la scène d'un coup sans toucher à chaque nœud.

## La convention de direction

`x` et `y` pointent **vers** la source. `(-0.35, -0.8)` = lumière en haut à gauche.
Ce n'est pas la direction dans laquelle la lumière voyage — c'est l'inverse, et
c'est la lecture naturelle de « négatif = à gauche ». Le vecteur n'a pas besoin
d'être unitaire, seule sa direction compte.

## Le piège qui décide de ta mise en page

**Le matériau suit la boîte, pas la géométrie d'un composant.** Il est clippé par
`border-radius` et par `clip-path`, jamais par ce qu'un `shape` dessine lui-même :
`shape` peint sa propre forme, et le pass de peinture n'y a pas accès.

Donc pour une bille, écris un `div` avec `border-radius: "50%"` — pas un
`shape: circle`, dont le matériau déborderait dans les coins du carré englobant.
Pour un octogone, `clip-path` en polygone. **Donne la silhouette à la boîte.**

## Ce que ça ne fait pas

Pas d'ombre portée entre éléments, pas d'occlusion, pas de reflet de l'un dans
l'autre : c'est un habillage de surface par nœud, pas un moteur de rendu. Pour
séparer les plans, c'est la profondeur de champ qu'il faut — voir
[depth-of-field.md](depth-of-field.md), qui lit la même `style.depth`.
