# Deux animations sur la même propriété : elles se composent

Empiler deux effets qui touchent la même propriété est légitime et produit un
résultat **combiné**, pas « le dernier gagne ». C'est le contrat du moteur, et il
n'est pas celui des animations CSS — d'où cette règle.

```json
"animation": [
  { "name": "fade_in", "duration": 0.6 },
  { "name": "pulse", "loop": true }
]
```

`fade_in` et `pulse` touchent tous les deux `opacity`. À un instant où `fade_in`
vaut `0.5` et `pulse` vaut `0.8`, l'opacité rendue est **0.40**, leur produit —
pas `0.8`.

## Comment chaque propriété se combine

| Comportement | Propriétés |
|---|---|
| **Produit** | `opacity`, `scale_x`, `scale_y` |
| **Somme** | `translate_x`, `translate_y`, `rotation`, `rotate_x`, `rotate_y` |
| **Dernière valeur écrite** | tout le reste : `blur`, `blur_x`, `blur_y`, `color`, `border_radius`, `font_size`, `width`, `height`, `gap`, `padding`, `stroke_width`, `letter_spacing`, `draw_progress`, `draw_start`, … |

Le regroupement se fait par **famille d'effets**, pas par entrée du tableau : tous
les presets sont résolus ensemble, toutes les `keyframes` ensemble, puis les
résultats sont combinés. Deux presets qui animent `opacity` se multiplient donc
entre eux avant même d'arriver là.

## La valeur neutre n'est pas « ne rien faire »

Pour une propriété qui se **compose**, `1` (produit) et `0` (somme) sont les
éléments neutres : les appliquer ou les sauter donne la même réponse. Aucune
subtilité.

Pour une propriété en **dernière-valeur-écrite**, c'est différent : `blur: 0` est
une valeur, pas une absence. Une deuxième animation qui ramène le flou à zéro doit
effacer le flou posé par la première.

C'est pour ça que ces propriétés ont une valeur au repos **négative** (`-1`) et pas
`0` : le moteur distingue « cette animation n'a pas touché la propriété » de
« cette animation l'a amenée à zéro ». Un garde du genre `if other.blur > 0.001`
confond les deux et laisse l'élément flou pour le reste de la scène — c'est le bug
que l'issue #322 a relevé.

## Si on veut vraiment qu'une seule gagne

Il n'y a pas de mot-clé pour ça. On borne les fenêtres pour qu'elles ne se
chevauchent pas :

```json
"animation": [
  { "name": "fade_in", "delay": 0.0, "duration": 0.6 },
  { "name": "pulse", "delay": 0.6, "duration": 1.2, "loop": true }
]
```

Hors de sa fenêtre, un effet ne contribue rien, donc la question de la composition
ne se pose plus. Voir [animation-completion-budget.md](animation-completion-budget.md)
pour le calcul des fenêtres.
