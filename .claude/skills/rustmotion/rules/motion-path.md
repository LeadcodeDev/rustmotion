# Rule: Motion Path

Pour faire suivre une trajectoire à un composant — une courbe, un arc, un tracé en S — utilise l'effet d'animation `motion_path` plutôt que d'empiler des `translate` successifs dans une `timeline`.

## La forme

```json
{
  "type": "shape",
  "shape": "circle",
  "fill": "#F68F2B",
  "position": "absolute",
  "x": 160,
  "y": 700,
  "style": {
    "width": "56px",
    "height": "56px",
    "animation": [{
      "name": "motion_path",
      "path": "M0,0 C300,-320 750,-320 1100,-60",
      "delay": 0.2,
      "duration": 2.4,
      "orient": true,
      "orient_offset": 90,
      "easing": "ease_in_out"
    }]
  }
}
```

| Champ | Rôle |
|---|---|
| `path` | Données de chemin SVG (`M`/`L`/`H`/`V`/`C`/`S`/`Q`/`T`/`A`/`Z`) — **la même syntaxe** que `shape: { "type": "path", "data": ... }` |
| `delay`, `duration` | Fenêtre temporelle, comme tout autre effet |
| `loop` | Reprend au début à la fin du parcours |
| `orient` | Oriente le composant selon la tangente |
| `orient_offset` | Correction d'angle, en degrés |
| `easing` | Appliqué à la progression **le long du chemin**. Défaut linéaire = vitesse constante sur la courbe |

## Les coordonnées sont des deltas

Le chemin est relatif à la position que le layout aurait donnée au composant. `M0,0` est donc son point de repos, pas le coin du device — même convention qu'`orbit`.

Concrètement : positionne le composant normalement (`x`/`y`, ou le flux), puis décris la trajectoire **depuis là**.

## `orient_offset` n'est pas décoratif

Un composant est orienté selon la tangente, et la tangente pointe dans le sens du parcours. Si ton visuel pointe naturellement vers le haut — une flèche, une icône de fusée, un curseur — il apparaîtra tourné de 90° sur un chemin horizontal. `orient_offset: 90` le corrige.

Vérifie l'orientation au repos de ton visuel avant de conclure que `orient` est cassé.

## Le validateur voit la trajectoire

La position résout en `transform`, donc `rustmotion validate --strict-anim` détecte un composant qui sort du cadre en suivant sa courbe, et nomme l'instant :

```
bbox: [2046, 700] -> [2102, 756]   (viewport: 1920x1080)
hint: at t=1.70s (57% of scene), animation transforms (tx=1886, ty=0, …)
      push the bbox out of the viewport
```

C'est la raison de préférer `motion_path` à une position calculée à la main : une trajectoire écrite en dur dans des keyframes reste vérifiable, mais tu perds l'orientation automatique et la vitesse constante le long de la courbe.

## Cas dégénérés

Un chemin vide ou impossible à parser est **rejeté au chargement**. Un chemin d'un seul point, ou de longueur nulle, tient la position avec une rotation nulle. Une `duration` négative ou nulle est rejetée par `validate`. Aucun de ces cas ne produit de `NaN`.
