# Rule: Régler une animation par caractère ou par mot

Les sept presets `char_*` (`char_scale_in`, `char_fade_in`, `char_wave`, `char_bounce`, `char_rotate_in`, `char_slide_up`, `char_blur_in`) partagent une même config. Six champs la règlent, tous optionnels, **tous par défaut au comportement historique** : un scénario existant ne bouge pas.

```json
{
  "type": "text",
  "content": "CASCADE",
  "style": {
    "font-size": 92,
    "animation": [{
      "name": "char_slide_up",
      "direction": "down",
      "distance": 1.6,
      "scale_from": 0.9,
      "duration": 0.5,
      "stagger": 0.035
    }]
  }
}
```

| Champ | Rôle | Défaut |
|---|---|---|
| `direction` | `up` / `down` / `left` / `right` — d'où l'unité arrive | `up` |
| `distance` | Multiplicateur du déplacement (0.5 serré, 1.85 marqué) | `1.0` |
| `scale_from` | Échelle de départ de chaque unité (0.82 = «pop», 0.92 = à peine) | absent |
| `jitter` + `seed` | Irrégularité déterministe du `stagger` | `0` |
| `ink_from` | Couleur de départ, converge vers `style.color` | absent |
| `blur` | Sigma de départ (`char_blur_in` seul) | `14` |

## Ce que chaque preset lit

`direction` et `distance` n'ont de sens que pour les presets dont le mouvement **est** une translation : `char_slide_up` et `char_blur_in`. Les autres (`scale_in`, `bounce`, `rotate_in`, `fade_in`, `wave`) n'ont pas d'axe de déplacement à rediriger et les ignorent.

`scale_from` se **compose** avec le preset au lieu de le remplacer — sauf sur `char_scale_in` et `char_bounce`, qui possèdent déjà leur propre courbe d'échelle et l'ignorent (deux échelles empilées se battent au lieu de se composer).

## Le nom `char_slide_up` ne contraint pas la direction

`char_slide_up` avec `"direction": "down"` fait tomber les lettres depuis le haut. Le nom est historique : c'est le preset «translation», `up` en est le défaut. Il n'existe pas de `char_slide_down`.

## `granularity` décide de ce qu'est une unité

`"granularity": "word"` anime des mots, `"char"` (défaut) des caractères. Un titre de 40 caractères animé en `char` avec `stagger: 0.05` prend 2 s à s'installer avant même sa `duration` — compte le nombre d'unités avant de choisir le `stagger`, ou passe en `word`.

## Rappel sur `char_blur_in`

Il passe par le même chemin de résolution que ses six frères : il **hérite** du `stagger` d'un conteneur parent et fonctionne dans une étape de `timeline`. (Ça n'a pas toujours été le cas.)
