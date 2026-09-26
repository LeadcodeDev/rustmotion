# Rule: Chromatic Aberration (per-élément)

`chromatic_wipe` (voir CLAUDE.md) sépare les canaux rouge/cyan **entre deux scènes**, comme composite de deux frame-buffers déjà rendus. `chromatic_aberration` fait la même séparation de canaux, mais comme effet d'animation sur **un seul nœud** — l'icône, la carte, le texte qui arrive avec un franc glitch chromatique avant de se stabiliser. Pas de transition, pas de deuxième frame : juste le contenu déjà peint de l'élément, dédoublé et décalé.

## La forme

```json
{
  "type": "icon",
  "name": "zap",
  "style": {
    "animation": [{
      "name": "chromatic_aberration",
      "amount": 6,
      "duration": 0.6
    }]
  }
}
```

| Champ | Rôle | Défaut |
|---|---|---|
| `delay` | Attente avant le début de la séparation (s) | `0` |
| `duration` | Temps pour revenir à une séparation nulle (s) | `0.6` |
| `amount` | Écart maximal entre les canaux, en px, atteint dès `delay` | `6` |
| `easing` | Courbe appliquée à la décroissance | `ease_out` |

## Ce n'est pas un aller-retour

Contrairement à `chromatic_wipe`, dont le pic se situe **au milieu** de la transition (nul aux deux bouts, pour ne pas laisser de frange sur la scène suivante), `chromatic_aberration` est **maximal dès le premier instant** (`delay`) et décroît vers zéro à `delay + duration`. C'est un effet d'arrivée — l'élément se matérialise dans un éclat chromatique puis se stabilise — pas un flash symétrique. Après `delay + duration`, l'élément est pixel pour pixel identique à un élément sans l'effet : aucune frange ne reste accrochée.

Si tu veux un flash qui pique au milieu plutôt qu'au début, compose deux `chromatic_aberration` avec des `delay` décalés ou pilote `amount` via ta propre courbe — le champ n'accepte qu'une seule forme de décroissance (pic au début).

## Comment c'est peint

Le contenu du nœud (fond, bordure, enfants, `shimmer`…) est peint une seule fois dans le layer d'opacité du nœud, puis un `ImageFilter` Skia recompose deux copies décalées de ce layer : une isolée sur le canal rouge décalée d'un côté, une isolée sur le canal cyan (vert+bleu) décalée de l'autre, sommées en mode `Plus`. Loin des bords de l'élément les deux copies se recouvrent et reconstituent la couleur d'origine exactement ; c'est seulement à la frontière — où l'une des deux copies échantillonne en dehors du contenu peint — qu'une frange colorée apparaît.

Conséquence pratique : c'est le même mécanisme que `style.filter` (blur, drop-shadow…) — un filtre d'image posé sur le layer du nœud — donc ça se compose avec un `style.filter` existant sur le même nœud, et ça hérite des mêmes règles de bleed :

- Un parent `overflow: hidden` clippe la frange, comme il clipperait un flou qui déborde.
- Le propre `overflow: hidden` du nœud ne la clippe **pas** — il ne clippe que les enfants, jamais le contenu du nœud lui-même (même règle que pour son ombre portée sortante).

## Piège : `amount`, pas `amplitude`

`float_3d` et les presets oscillants voisins utilisent `amplitude` pour leur intensité en px. `chromatic_aberration` n'est pas un preset de la même famille — c'est un effet à config dédiée comme `shimmer`, `glow` ou `wiggle` — et son champ s'appelle **`amount`**. `amplitude` sur `chromatic_aberration` est un champ inconnu, rejeté par le schéma.

De la même façon, `chromatic_aberration` ne prend ni `spring`, ni `overshoot`, ni `loop` / `repeat` : ce sont des champs de `AnimationTiming`/`PresetConfig` (la famille `fade_in`, `scale_in`, …), pas de cette config.

## Cas dégénérés

- `amount: 0` (ou négatif au point de devenir imperceptible) : aucune frange visible, pas d'erreur — l'effet est simplement inerte.
- `amount` négatif mais non nul : la séparation est réelle, seuls les côtés rouge/cyan s'inversent.
- `duration: 0` (ou négative) : l'effet ne s'active jamais — traité comme absent à chaque frame.
- `delay` seul ne rejoue jamais l'effet : il n'y a pas de `loop` ici, une seule décroissance par nœud.
