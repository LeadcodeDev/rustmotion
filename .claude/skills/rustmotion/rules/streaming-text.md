# Rule: Texte qui « streame » (arrivée de tokens)

Pour figurer une réponse de modèle en train de s'écrire, **n'utilise pas `typewriter`** : un typewriter révèle caractère par caractère à cadence fixe, ce qui lit comme une machine à écrire, pas comme un flux de tokens. Un modèle émet des mots entiers, par bouffées inégales, et chaque mot s'installe visuellement au lieu d'apparaître net d'un coup.

```json
{
  "type": "text",
  "content": "Les mots arrivent par bouffées inégales, comme des tokens.",
  "style": {
    "font-size": 40,
    "color": "#E2E8F0",
    "animation": [{
      "name": "char_blur_in",
      "granularity": "word",
      "duration": 0.28,
      "stagger": 0.09,
      "jitter": 0.7,
      "seed": 12,
      "ink_from": "#475569",
      "blur": 6
    }]
  }
}
```

Trois champs font tout le travail :

- **`granularity: "word"`** — l'unité est le mot, pas la lettre.
- **`jitter`** — décale le départ de chaque unité de ±`jitter × stagger`. C'est ce qui casse la cadence métronomique. 0.5–0.8 lit comme du streaming ; au-delà de 1.0 les mots se croisent et l'ordre de lecture se brouille.
- **`ink_from`** — chaque mot démarre dans cette couleur et converge vers `style.color` sur sa durée. Un gris désaturé reproduit le token « pas encore accepté par l'œil ».

## Le `jitter` est déterministe, pas aléatoire

Les décalages sont dérivés de `seed` et de l'index de l'unité, jamais d'un RNG. C'est une contrainte, pas un détail : les frames sont rendues dans le désordre, en parallèle, et parfois dans des processus séparés (`--frames a-b`). Un mot dont le départ dépendrait d'un tirage sauterait entre deux frames voisines.

Changer `seed` rebat le rythme sans en changer la statistique — utile pour que deux paragraphes voisins ne « respirent » pas à l'identique.

Aucune unité ne peut démarrer avant le `delay` de l'effet : un décalage négatif sur la première unité la ferait apparaître à moitié animée dès la frame 0.

## Budget

`stagger × nombre de mots + duration` est le temps total d'installation. Sur une phrase de 12 mots avec `stagger: 0.09`, c'est ~1.4 s — vérifie que la scène est assez longue, `validate` le signale sinon.
