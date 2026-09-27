# `vhs` : la déchirure de bande

Un effet de scène (`scene.effects`) qui casse l'image en bandes horizontales
décalées latéralement, avec bruit, lignes de balayage et une ligne de suivi qui
descend. C'est le vocabulaire glitch d'un « rewind ».

```json
"effects": [
  { "type": "vhs", "at": 0.4, "duration": 0.8,
    "bands": 12, "offset": 60, "noise": 0.3, "scanlines": 0.2,
    "tracking_line": { "color": "#3DA5FF", "speed": 1.5, "thickness": 3 },
    "seed": 3 }
]
```

| Champ | Défaut | Rôle |
|---|---|---|
| `at` | — | début, sur la timeline **de la scène** (comme `flash`) |
| `duration` | `0.5` | durée en secondes |
| `bands` | `12` | nombre de bandes horizontales |
| `offset` | `40` | décalage latéral maximal d'une bande, en pixels |
| `noise` | `0.25` | intensité du bruit, 0..1 |
| `scanlines` | `0.2` | assombrissement une ligne sur deux, 0..1 |
| `tracking_line` | absent | la ligne colorée qui descend |
| `seed` | `1` | même graine, même déchirure |

## C'est un beat, pas un état

`at` et `duration` le bornent comme `flash`, et pour la même raison : la
référence le tient **moins d'une seconde**. Hors de sa fenêtre, la frame n'est pas
touchée du tout — pas « à peine », pas du tout. Un `vhs` qui couvre toute une
scène ne lit plus comme un accident de lecture, il lit comme un filtre.

## Deux choses à savoir

**Le redécoupage est temporel.** Les bandes sont retirées au sort douze fois par
seconde, pas à chaque frame : une déchirure qui change à 30 ou 60 images par
seconde scintille au lieu de s'accrocher. La graine et l'instant décident
ensemble, donc deux rendus du même fichier donnent les mêmes octets.

**Une bande décalée laisse du vide.** Le bord libéré est rempli en noir, pas
répété ni étiré — c'est ce qui fait lire le décalage comme une déchirure et non
comme un flou. Sur un fond clair ça se voit beaucoup ; baisse `offset` plutôt que
d'espérer que ça se fonde.

## Ce que ça ne remplace pas

`chromatic_wipe` sépare les canaux à une **coupe**, `chromatic_aberration` sur un
**nœud**. `vhs` est un état de la frame entière. Les combiner sur le même beat est
possible et c'est souvent ce que fait la référence — le `vhs` porte la géométrie,
l'aberration porte la couleur.
