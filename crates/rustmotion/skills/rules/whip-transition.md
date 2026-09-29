# Rule: `whip` — le pan flouté directionnel

`whip` est une `transition` (au même titre que `slide`, `chromatic_wipe`, `zoom_blur`…) : comme toute transition d'une vue `slide`, elle composite deux frame-buffers **déjà rendus** — aucun élément ne survit à la coupe, seuls les pixels sont mélangés. Voir la section « Composition » de `CLAUDE.md`.

L'effet : un `slide` classique le long d'un axe, dont le déplacement porte un filé de mouvement qui culmine à mi-transition — la scène sortante s'étire en traînée derrière elle le long de `direction` en s'estompant, et la scène entrante arrive déjà striée avant de se stabiliser, nette, à l'arrivée.

```json
{
  "transition": {
    "type": "whip",
    "direction": "left",
    "strength": 1.5,
    "duration": 0.35,
    "easing": "ease_in_out"
  }
}
```

(exemple de placement — `transition` se pose entre deux scènes d'une vue `slide`, comme n'importe quelle autre transition)

## Ne pas confondre avec `slide`, `chromatic_wipe` ni l'effet `motion_blur`

- **`slide`** est le même déplacement, sec, sans traînée : `whip` avec `strength: 0` lui est byte-identique à chaque instant de la transition.
- **`chromatic_wipe`** voyage sur le même axe (`direction` prend les mêmes valeurs), mais son pic est une séparation **chromatique** (rouge/cyan) sur le bord de coupe, pas un filé spatial de la frame entière.
- **`motion_blur`** (effet d'animation, `style.animation`) traîne la trajectoire d'un **composant individuel** en accumulant des échantillons de sa propre animation. Il ne voit rien dans une transition, qui ne dispose plus que de deux buffers RGBA déjà peints — c'est exactement le trou que `whip` bouche côté transition, comme `zoom_blur` l'a fait pour le zoom radial. Voir [rules/zoom-blur-transition.md](zoom-blur-transition.md).

## Champs

| Champ | Rôle | Défaut |
|---|---|---|
| `direction` | Axe de déplacement des deux frames, mêmes valeurs que `slide`/`chromatic_wipe` (`left`/`right`/`up`/`down`). | `left` |
| `strength` | Portée de la traînée. `0` supprime la passe de flou et laisse un `slide` sec — pas de traînée à aucun instant, même à mi-transition. Les valeurs plus grandes tirent les copies plus loin derrière leur position courante. | `1.0` |
| `duration`, `easing` | Communs à toutes les transitions. | `0.5`, `ease_in_out` |

## Zéro aux deux bouts, par construction

Comme `zoom_blur` et `chromatic_wipe`, l'intensité suit `peak = 1 - |2p - 1|` : nulle à `progress = 0` et à `progress = 1`, quelle que soit `strength`. Le moteur ne laisse pas cette courbe tendre vers zéro : à `reach <= 0.0` (donc `peak == 0`, aux deux bornes), il retourne directement le slide net, sans jamais construire la passe de traînée — un court-circuit, pas une atténuation flottante qui pourrait laisser un résidu d'arrondi. `progress = 0` rend exactement la frame source, `progress = 1` exactement la frame de destination : rien ne bave sur la scène suivante.

`strength: 0` prend le même court-circuit à **tout instant** de la transition, pas seulement aux bords : la transition dégénère alors en un `slide` sec, sans jamais poser la passe de traînée.

## Comment c'est construit

Le socle net (`sharp`) est le même calcul que le `slide` interne de `chromatic_wipe` — factorisé dans `directional_slide` et partagé par les deux transitions : les deux frames pleinement opaques, carrelées côte à côte le long de `direction`, sans aucun mélange alpha. Quand `strength` et la position dans la transition l'exigent, une dizaine de copies translatées de **chaque** frame — la sortante ET l'entrante, contrairement à `zoom_blur` qui ne traîne que la frame sortante — sont redessinées de plus en plus loin derrière leur position courante, à une opacité qui décroît avec la distance. C'est la même somme de copies pondérées que `zoom_blur`, translatée le long d'un axe au lieu d'être mise à l'échelle radialement autour d'un `origin`.

## Piège : une `strength` élevée fait déborder le fantôme sur l'autre scène

Les copies translatées sont dessinées sur tout le cadre, pas seulement dans le territoire qui appartient encore à leur propre frame. Une `strength` très élevée fait donc bleeder un fantôme semi-transparent de la scène sortante dans la zone déjà occupée par l'entrante, et réciproquement — c'est voulu, c'est précisément ce qui donne l'impression d'un filé de mouvement qui traverse la coupe plutôt que deux images qui glissent l'une sur l'autre. Si l'effet paraît trop étalé, baisser `strength` plutôt que `duration` : raccourcir la durée ne change rien au pic de `peak`, seulement la vitesse à laquelle la transition le traverse.
