# Rule: `number_wheel` vs `counter`

Deux composants affichent un nombre qui s'anime. Ils ne racontent pas la même chose.

**`counter`** interpole une *valeur* et réécrit le nombre à chaque frame. Il répond à « combien, en ce moment ? » — une jauge qui monte, un total qui se cumule. Ses glyphes sautent, parce que 8 999 puis 9 000 n'ont rien en commun.

**`number_wheel`** fait défiler des bandes de chiffres, comme un compteur mécanique. Il répond à « la figure atterrit » — un KPI qui se pose, un résultat qui se révèle. Ce qu'on regarde, c'est le mouvement ; ce qui reste, c'est le chiffre demandé.

```json
{
  "type": "number_wheel",
  "value": "30,222",
  "spin": "double",
  "duration": 1.1,
  "delay": 0.3,
  "stagger_per_column": 0.09,
  "style": { "font-size": 120, "font-weight": 700, "color": "#38BDF8" }
}
```

| Champ | Rôle | Défaut |
|---|---|---|
| `value` | La figure telle qu'écrite : `"30,222"`, `"5.7"`, `"98%"` | requis |
| `spin` | `single` / `double` / `triple` — tours de 0-9 avant l'atterrissage | `single` |
| `duration` | Durée d'atterrissage **d'une** roue | `1.2` |
| `delay` | Avant le départ de la première roue | `0` |
| `stagger_per_column` | Décalage par colonne, de gauche à droite | `0.08` |
| `easing` | Courbe du trajet | `ease_out_cubic` |

## `value` est une chaîne, pas un nombre

Les chiffres roulent ; tout le reste — virgule, point, signe, unité — est peint à sa place, immobile. C'est ce qui permet d'écrire `"1 204 €"` ou `"98%"` sans que le séparateur ne parte en vrille.

## `spin` change la vitesse, pas la durée

Chaque roue prend `duration` quoi qu'il arrive. `triple` ne rend pas l'animation plus longue : il fait défiler trois fois plus de chiffres dans le même temps. Un `triple` sur une `duration` courte devient une bouillie illisible.

## `stagger_per_column: 0` est un défaut à éviter

Toutes les roues atterrissent alors ensemble, ce qui lit comme un simple flip. Le décalage gauche→droite est ce qui fait que le dernier chiffre est celui qui *règle* la figure.

## La boîte réserve la place du chiffre le plus large

Chaque colonne fait la largeur du chiffre le plus large de la police, pas celle du chiffre final : sinon un `111` réserverait une boîte étroite puis déborderait pendant qu'un `0` défile. Le validateur mesure la même chose.
