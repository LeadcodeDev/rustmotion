# Rule: `burst` — l'éclaboussure de traits autour d'un élément qui apparaît

`burst` (`style.animation`) peint une couronne de traits courts qui partent vers l'extérieur juste au large de la boîte du nœud, puis se résorbent. C'est l'accent qu'on met sur une pastille, un badge ou une coche au moment où elle *pop* — l'équivalent graphique du petit « tchac ».

```json
{
  "type": "badge",
  "text": "Livré",
  "style": {
    "animation": [
      { "name": "pop_in", "delay": 0.2, "duration": 0.45 },
      { "name": "burst", "delay": 0.28, "duration": 0.4, "count": 10,
        "length": 34, "gap": 10, "width": 4, "color": "#FFB020",
        "seed": 3, "jitter": 0.25 }
    ]
  }
}
```

| Champ | Rôle | Défaut |
|---|---|---|
| `delay` | Attente avant le départ des traits (s) | `0` |
| `duration` | Durée totale aller-retour (s) ; la tête atteint le bout de sa course à mi-parcours | `0.4` |
| `count` | Nombre de traits dans la couronne (borné en interne à `1..=64`) | `8` |
| `length` | Longueur de la course de chaque trait, en px, mesurée à partir de `gap` | `40` |
| `gap` | Distance en px entre le bord de la boîte et le départ de chaque trait | `12` |
| `width` | Épaisseur du trait en px | `4` |
| `color` | Couleur du trait (chaîne hex) | `"#FFB020"` |
| `seed` | Graine du jitter d'angle, de longueur et de phase | `0` |
| `jitter` | Écart maximal d'un trait par rapport à sa part régulière de la couronne, en fraction de l'espacement entre deux traits ; module aussi la longueur et la phase | `0.2` |

## La tête part, la queue rattrape

Chaque trait est défini par deux extrémités qui parcourent la même piste une fois chacune : la **tête** sort sur la première moitié de la fenêtre (`ease_out`), la **queue** la suit sur la seconde (`ease_in`). Le trait s'allonge, atteint sa pleine longueur à mi-parcours, puis se referme **vers l'extérieur** — il s'envole et disparaît, il ne rentre pas dans la boîte.

Conséquence directe : aux deux bornes de la fenêtre, tête et queue sont au même endroit, donc le trait a une longueur nulle. **La garantie « zéro aux deux bouts » est géométrique ici, pas seulement temporelle.** C'est une nuance qui compte : `burst_progress` court-circuite bien en dehors de `[delay, delay + duration)`, mais même si une frame tombait *dans* la fenêtre à un ULP près de sa borne (ce qui arrive : `delay + duration - delay != duration` en flottant dès que `duration` n'est pas représentable exactement, `0.4` par exemple), le trait mesuré serait de longueur nulle et rien ne serait peint. Les deux protections existent, et elles ne couvrent pas le même cas.

## Rien de ce qui appartient au nœud n'est touché

Les traits sont peints **par-dessus** le nœud, **hors de sa boîte**, après son propre rendu, à l'intérieur de sa transformation. Trois conséquences :

- Ils ne prennent **aucune place dans le layout** — un `burst` ne pousse jamais un voisin en flex.
- Ils suivent le nœud : si celui-ci tourne ou se déplace (`pop_in`, `transform`), la couronne tourne et se déplace avec lui.
- `gap` garantit que rien ne mord sur la boîte. `gap: 0` colle les traits au bord ; une valeur négative est ramenée à `0`, jamais un chevauchement.

La couronne peut en revanche sortir du **viewport** si le nœud est près d'un bord. Le validateur de géométrie ne la voit pas (il inspecte les boîtes de layout, et `burst` n'en a pas) : c'est à la mise en page de laisser `gap + length` de marge autour du nœud.

## Le budget d'animation s'applique

Comme [shatter.md](shatter.md), `burst` entre dans le calcul de [animation-completion-budget.md](animation-completion-budget.md) : `start_at + delay + duration ≤ scene_duration`. Ce n'est pas un preset de sortie exempté.

En pratique on le déclenche **légèrement après** l'entrée qu'il accentue, pas en même temps : l'éclaboussure doit répondre au *pop*, pas le précéder. Dans l'exemple ci-dessus, `pop_in` part à `0.2` et `burst` à `0.28`.

## Cas dégénérés

- `count: 0` est traité comme `1` — un unique trait, ce qui ressemble davantage à un accident qu'à un éclat.
- `length: 0` ou `width: 0` n'affiche rien du tout : il n'y a pas d'erreur, l'effet est simplement inerte.
- `jitter: 0` donne une couronne parfaitement régulière, qui lit comme un soleil de schéma technique ; `jitter: 1` autorise un trait à empiéter sur le créneau de son voisin, ce qui lit comme une projection. Entre les deux, `0.2` à `0.35` est la plage qui a l'air « dessinée à la main ».
- `duration` nulle ou négative désactive l'effet à chaque frame, comme `chromatic_aberration` et `shatter`.

## Ce n'est pas `emitter`

`burst` est **borné** : un aller-retour, `count` traits, puis plus rien. `emitter` ([emitter-lifecycle.md](emitter-lifecycle.md)) est un **flux continu** : des particules naissent, voyagent, meurent et renaissent tant que la scène dure. Un badge qui apparaît → `burst`. Un tunnel de warp ou un champ d'étoiles → `emitter`.
