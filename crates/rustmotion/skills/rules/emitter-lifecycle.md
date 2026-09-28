# `emitter` : un champ de particules avec un cycle de vie

Pour un tunnel de warp, un champ d'étoiles, un flux de lumière ou de braises —
n'importe quel champ de petites marques en mouvement qui doit paraître **vivant
et continu**, pas une cohorte figée qui apparaît d'un bloc.

`emitter` remplace le `particle` déprécié et la recette artisanale
`for-each` + `rand($seed, $i)` + `sin($t)` : aucune des deux n'a de cycle de vie,
donc elles gèlent à un seul âge pour tout le plan, ou demandent des centaines de
nœuds keyframés à la main. Une étude réelle a généré **460 nœuds** et des
centaines de kilo-octets de JSON pour trois secondes de tunnel.

```json
{
  "type": "emitter",
  "origin": { "x": 960, "y": 540 },
  "rate": 180,
  "life": [0.7, 1.2],
  "direction": "radial",
  "speed": { "from": 200, "to": 1600, "easing": "ease_in" },
  "spawn_radius": [260, 470],
  "shape": "streak",
  "length": [60, 240],
  "color": "#EEF4FF",
  "width": 3,
  "seed": 7
}
```

| Champ | Rôle |
|---|---|
| `origin` | `{x, y}` dans la boîte de l'émetteur, en pixels. Défaut : son centre. |
| `rate` | Particules nées par seconde, en moyenne. |
| `life` | `[min, max]` en secondes. Chaque particule tire la sienne une fois, depuis `seed` et son index. |
| `direction` | `radial` uniquement pour l'instant : naissance sur un anneau, trajet droit vers l'extérieur. |
| `speed` | `{from, to, easing}` — pixels/seconde à la naissance et à la mort, et comment le trajet se répartit sur la vie. |
| `spawn_radius` | `[min, max]` de l'anneau de naissance, en pixels depuis `origin`. **Un minimum non nul est ce qui creuse l'œil sombre** au centre d'un tunnel. |
| `shape` | `streak` (un trait aligné sur la direction) ou `dot`. |
| `length` | `[min, max]` de la longueur du trait. Ignoré pour `dot`. |
| `color` | Chaîne hexadécimale. |
| `width` | Épaisseur du trait, ou diamètre du point. |
| `seed` | Graine. Même graine, même instant, mêmes pixels — toujours. |

## Il n'y a pas de nombre de particules à régler

`rate` et `life` suffisent. Combien de particules vivent à un instant donné en
découle : `concurrence = rate × moyenne(life)`. Un `rate: 180` avec
`life: [0.7, 1.2]` (moyenne 0,95 s) garde environ **171** particules vivantes en
continu.

C'est délibéré : un champ `count` séparé serait une troisième valeur à tenir
d'accord avec les deux autres, et la première chose à désynchroniser en réglant
l'une sans l'autre.

## Le cycle de vie est en forme close, pas simulé

L'âge de chaque particule se déduit directement de `(seed, index, time)` :

```
phase[i]  = décalage aléatoire dans sa propre vie, tiré une fois depuis seed et i
age(t)    = (t + phase[i]) mod life[i]
progress  = age / life[i]            // 0 à la naissance, →1 à la mort
```

Rien n'avance de frame en frame, aucun historique n'est gardé. **C'est ce qui rend
l'émetteur cherchable** : `still --time 1.7` produit exactement les pixels de la
frame 51 d'un `render` complet, parce que les deux appellent la même fonction pure
du temps. Une simulation, elle, dépendrait de tout ce qui précède — et `still` n'a
rien qui précède.

C'est aussi ce qui empêche le champ de paraître synchronisé : deux particules ne
partagent une phase que si leurs tirages se rencontrent, donc le tunnel lit comme
un flux d'âges mélangés **dès la première frame**, pas comme une cohorte née à
`t=0`.

`speed.from`/`speed.to` décrivent la vitesse moyenne sur toute la vie ;
`speed.easing` décide ensuite comment cette distance totale se répartit sur
`progress` — `ease_in` en dépense l'essentiel vers la fin, ce qui lit comme une
accélération vers l'extérieur. Chaque particule s'estompe brièvement à la
naissance et avant la mort, pour qu'aucune apparition ne claque.

## C'est un composant décoratif, plein cadre

Comme `particle`, il vaut `100%` en largeur et hauteur par défaut et il est
**décoratif** : il peint en couche plein écran derrière le flux flex de la scène,
et il est exempté du contrôle de débordement viewport — un tunnel est censé
déborder. Donne-lui un `style.width`/`style.height` explicite pour le contenir
dans une carte.

## Un plafond, et pourquoi

Le nombre de particules est plafonné à 6000. `rate` est piloté par l'auteur et
multiplie directement les appels de dessin par frame ; sans plafond, une valeur
aberrante n'échoue pas, elle fait ramer le rendu sans rien dire.

## `particle` n'est pas étendu pour ça

Ses cinq presets (`confetti`, `snow`, `stars`, `bubbles`, `halo`) restent là,
dépréciés, pour la compatibilité. Les compositions figées qu'ils dessinent n'ont
aucune sémantique de naissance, mort et renaissance ; en ajouter une reviendrait
à réécrire `emitter` avec une indirection de plus.
