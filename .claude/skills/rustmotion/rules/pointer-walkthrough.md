# Rule: Curseur de souris simulé (`pointer`)

Pour une démo produit ou un walkthrough d'agent — la flèche qui se déplace vers un contrôle et clique dessus — utilise `pointer`.

**`cursor` n'est pas ça.** `cursor` est un caret texte : une barre verticale clignotante. Son champ `cursor_style: "pointer"` est de la métadonnée morte, il dessine une barre dans les deux cas.

```json
{
  "type": "pointer",
  "position": "absolute",
  "x": 0,
  "y": 0,
  "size": 52,
  "tone": "light",
  "click_ring": "bold",
  "ring_color": "#38BDF8",
  "click_duration": 0.5,
  "path": [
    { "time": 0.4, "x": 1500, "y": 820 },
    { "time": 2.0, "x": 480,  "y": 330 },
    { "time": 3.6, "x": 900,  "y": 690 }
  ]
}
```

| Champ | Rôle |
|---|---|
| `size` | Hauteur de la flèche en px. L'anneau de clic suit. |
| `tone` | `light` (flèche blanche, contour sombre) ou `dark` |
| `color` / `outline_color` | Surchargent `tone` |
| `click_ring` | `subtle` / `standard` / `bold` / `none` |
| `path` | Waypoints `{time, x, y}` — le pointeur **clique en arrivant** sur chacun |
| `click_at` | Clics d'un pointeur immobile. **Ignoré si `path` est présent** |
| `click_duration` | Durée du clic, *et* pause sur le waypoint avant de repartir |
| `path_easing` | `ease_in_out` (défaut), `linear`, `ease_out`, `step` |

## Les coordonnées partent de l'origine du composant

`x`/`y` d'un waypoint sont relatifs à la boîte du `pointer`, pas au device. Place le composant en `position: absolute, x: 0, y: 0` et les waypoints se lisent alors comme des coordonnées de scène — c'est la forme à privilégier pour un walkthrough.

## La boîte est le glyphe, pas le parcours

La boîte du composant fait la taille de la flèche : ce sont les waypoints qui la translatent. Dimensionner la boîte au parcours ferait pousser les frères d'un `flex` par un élément qui n'est qu'un curseur.

Corollaire : `pointer` est **exempté du contrôle de débordement viewport**, comme `marquee` et `cursor`. Une démo qui amène la flèche près d'un bord met légitimement sa queue dehors.

## Le déplacement fait une pause sur le clic

Entre deux waypoints, le pointeur ne repart qu'une fois l'animation de clic finie (`click_duration`). C'est ce qui rend le geste lisible : arriver, cliquer, repartir. Un `click_duration` proche de l'écart entre deux waypoints laisse à peine le temps du trajet — laisse au moins le double.
