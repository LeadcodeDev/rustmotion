# Incliner tout le plan : `camera.rotate_x` / `rotate_y` / `perspective`

La parallaxe de `style.depth` connaît la translation et le zoom. Pour faire
basculer un plan entier, on copiait jusqu'ici les mêmes keyframes `rotate_x` sur
chaque groupe — et **chaque groupe se retrouvait avec son propre point de fuite**,
ce qui ne lit pas comme une caméra mais comme des cartes qui tournent chacune dans
son coin.

```json
"camera": {
  "perspective": 1400,
  "keyframes": [
    { "property": "rotate_y",
      "values": [{ "time": 0, "value": -16 }, { "time": 2, "value": 16 }],
      "easing": "ease_in_out" }
  ]
}
```

| Champ | Défaut | Rôle |
|---|---|---|
| `rotate_x` | `0` | bascule autour de l'axe horizontal, en degrés |
| `rotate_y` | `0` | bascule autour de l'axe vertical |
| `perspective` | `0` | distance d'observation en pixels ; `0` = projection orthographique |

Les trois s'animent par `keyframes` comme `zoom` et `rotation`.

## Un seul point de fuite, et l'échelle par profondeur

La perspective est appliquée **une fois**, autour de l'origine de la caméra. C'est
toute la différence avec la version copiée sur chaque groupe.

La rotation, elle, est mise à l'échelle par le `style.depth` de chaque enfant
direct de la scène — la même règle que la parallaxe et que `camera.focus` suivent
déjà. Un plan à `depth: 3` bascule trois fois plus qu'un plan à `depth: 1`, ce qui
est ce qui donne la sensation de volume.

## Deux pièges

**Ça ne s'applique qu'aux enfants directs de la scène.** Comme la parallaxe : un
plan est une couche. Poser `depth` sur un nœud enfoui dans un sous-arbre ne le
fait pas basculer tout seul.

**`perspective: 0` n'est pas « pas de rotation ».** C'est une bascule
orthographique — la forme se déforme sans converger. Pour une caméra qui lit comme
une caméra, il faut une distance, et `1200`–`1800` couvre la plupart des cadrages.

## Ce qui n'y est pas

Le flou de mouvement de caméra (#362 partie 2) n'est pas ici. Il demande
d'accumuler plusieurs rendus sous-frame le long du mouvement, ce qui vit dans la
boucle d'encodage et non dans le pass de peinture.
