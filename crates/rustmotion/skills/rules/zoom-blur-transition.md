# Rule: `zoom_blur` — la coupe "tunnel"

`zoom_blur` est une `transition` (au même titre que `fade`, `slide`, `chromatic_wipe`…) : comme toute transition d'une vue `slide`, elle composite deux frame-buffers **déjà rendus** — aucun élément ne survit à la coupe, seuls les pixels sont mélangés. Voir [rules/motion-path.md](motion-path.md) et la section « Composition » de `CLAUDE.md` pour ce que ça implique.

L'effet : la scène sortante s'étire radialement vers l'extérieur depuis un point central en s'estompant — l'impression d'être aspiré dans un tunnel — puis la scène entrante apparaît, nette.

```json
{
  "type": "chromatic_wipe",
  "transition": {
    "type": "zoom_blur",
    "strength": 1.5,
    "duration": 0.5
  }
}
```

(exemple de placement — `transition` se pose entre deux scènes d'une vue `slide`, comme n'importe quelle autre transition)

## Ne pas confondre avec `zoom_in` ni avec l'effet `motion_blur`

- **`zoom_in`** (transition) fait la même bascule d'échelle mais sur une image **nette** : pas de traînée, juste un zoom sec.
- **`motion_blur`** (effet d'animation, `style.animation`) traîne la trajectoire d'un **composant individuel** en accumulant plusieurs échantillons temporels de sa propre animation (`intensity`, `samples`, `shutter` — voir `crates/rustmotion-core/src/schema/video.rs`). Il ne peut rien faire ici : une transition ne voit plus de composants, seulement deux buffers RGBA déjà peints. C'est précisément pour boucher ce trou que `zoom_blur` existe comme **transition** et non comme effet.

## Champs

| Champ | Rôle | Défaut |
|---|---|---|
| `strength` | Portée des traînées. `0` supprime le passage de flou et ne laisse qu'un zoom sec — pas de traînée à aucun instant, même à mi-transition. Les valeurs plus grandes tirent les copies externes plus loin de `origin`. | `1.0` |
| `origin` | `{ "x": …, "y": … }`, le point (en pixels du cadre) d'où les traînées rayonnent. Absent = centre du cadre. | absent → centre |
| `duration`, `easing` | Communs à toutes les transitions. | `0.5`, `ease_in_out` |

`origin` prend la même forme que `CameraOrigin` (`camera.origin`) — deux champs `x`/`y` en pixels, pas de `%`.

## Zéro aux deux bouts, par construction

Comme `chromatic_wipe`, l'intensité de l'effet suit une courbe qui vaut zéro à `progress = 0` et à `progress = 1` (`peak = 1 - |2p - 1|`, le même calcul que pour l'aberration chromatique). Le moteur ne se contente pas de laisser cette courbe tendre vers zéro : à `reach <= 0.0` (donc quand `peak == 0`, c'est-à-dire aux deux extrémités, **quelle que soit la valeur de `strength`**), il retourne directement le composite sans traînée — un court-circuit, pas une atténuation flottante qui pourrait laisser un résidu d'arrondi. `progress = 0` rend exactement la frame source, `progress = 1` exactement la frame de destination : rien ne bave sur la scène suivante.

`strength: 0` prend le même court-circuit à **tout instant** de la transition, pas seulement aux bords — dans ce cas la transition dégénère en un zoom sec identique à `zoom_in`, sans jamais construire la passe de traînées.

## Comment c'est construit

Le composite "net" (`sharp`) est un zoom classique : la frame sortante est mise à l'échelle autour de `origin` et s'estompe (`alpha = 1 - progress`) au-dessus de la frame entrante, dessinée pleine et immobile en dessous. Quand `strength` et la position dans la transition l'exigent, une seconde passe redessine la frame sortante une dizaine de fois à des échelles croissantes autour du même `origin`, avec une opacité qui décroît à mesure que l'échelle grandit — la technique suggérée par l'issue d'origine : une somme de copies mises à l'échelle, échantillonnées le long d'un rayon partant du centre. Aucun tirage aléatoire nulle part : deux rendus de la même frame produisent des octets identiques.

## Piège : un bord qui passe par `origin` ne peut pas se voir flouter

Le flou radial est une mise à l'échelle autour d'un pivot. Un point qui se trouve exactement sur ce pivot ne bouge sous **aucune** échelle — donc si le contenu qui doit sembler s'étirer a un bord qui coïncide avec `origin` (typiquement : un dégradé pile au centre du cadre, avec `origin` par défaut), ce bord précis restera net quel que soit `strength`. Ce n'est pas un bug du moteur, c'est la géométrie d'un zoom : décale `origin` du point que tu veux voir s'étirer, ou vérifie l'effet sur un contenu qui n'est pas parfaitement centré sur le pivot.
