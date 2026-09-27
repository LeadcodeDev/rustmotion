# `material: "inflated"` : un volume, pas un vernis

Les trois presets de [material-and-light.md](material-and-light.md) calculent leur
reflet sur la **boîte** du nœud, puis le clippent. Sur une étoile découpée dans un
rectangle, ça donne une seule bande de lumière en travers de la boîte — pas un
relief par branche. L'objet reste une surface plate vernie.

`inflated` calcule l'ombrage à partir de la **silhouette elle-même**.

```json
"style": {
  "clip-path": { "kind": "polygon", "points": [ … une étoile … ] },
  "material": { "preset": "inflated", "bevel": 24, "softness": 0.6 }
}
```

| Champ | Défaut | Rôle |
|---|---|---|
| `bevel` | `18` | distance en pixels sur laquelle le bord s'arrondit vers l'intérieur |
| `softness` | `0.6` | profil du bord, 0..1 — près de 0 un chanfrein net, près de 1 un coussin |

`intensity` et la `light` de la scène agissent comme pour les autres presets :
`intensity: 0` rend exactement la forme nue.

## Comment ça marche, parce que ça décide des réglages

La silhouette est rastérisée puis floutée ; le **dégradé du masque flouté est la
normale de surface**. On l'éclaire avec la direction de `scene.light`. Chaque
branche a son propre bord, donc chacune reçoit sa propre lumière et son propre
creux.

Conséquence directe : `bevel` est à la fois la largeur du flou **et** l'échelle à
laquelle la pente est mesurée. Un grand `bevel` sur une petite forme aplatit tout
— le flou noie la silhouette avant d'avoir pu en tirer une normale. Compte un
`bevel` d'au plus un quart de la plus petite dimension de la forme.

## Trois choses à savoir

**Ça coûte des pixels.** Contrairement aux autres presets, qui sont trois
dégradés Skia, celui-ci rastérise, floute et parcourt le masque à chaque frame.
Sur un nœud plein écran, ça se sent. C'est fait pour des étoiles, des pastilles et
des icônes, pas pour un fond.

**Il ignore `highlight` et `edge`.** Il n'a ni reflet spéculaire ni arête : sa
lumière vient entièrement de la géométrie. Le poser à côté d'un `glossy` mélange
deux modèles d'éclairage — ça peut être voulu, mais ce n'est pas une variante de
réglage.

**Donne la silhouette à la boîte.** Comme pour les autres presets, c'est
`clip-path` ou `border-radius` qui définit la forme, jamais la géométrie d'un
composant `shape`, que le pass de peinture ne voit pas. Sans `clip-path`, une
boîte carrée donne un bourrelet sur ses quatre bords — correct, mais sans intérêt.
