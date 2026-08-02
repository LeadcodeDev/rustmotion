# Parallaxe des cards

**Règle : dès qu'une scène contient plusieurs cards, elles doivent dériver
lentement en vertical les unes par rapport aux autres.** Une card seule à
l'écran n'a rien contre quoi faire parallaxe : elle reste immobile.

Ce qui distingue la profondeur de la chorégraphie n'est pas le mouvement
lui-même mais sa **divergence**. Des cards qui montent et descendent ensemble
lisent comme une danse — c'est le défaut le plus visible et le plus vite
fatigant. Les mêmes cards sur des phases et des amplitudes différentes lisent
comme des plans à des distances différentes.

## Les deux leviers

`float_3d` en `loop` expose trois champs qui doivent **tous** varier d'une card
à l'autre :

| champ | rôle | plage utile |
|---|---|---|
| `amplitude` | course verticale en px — le plan proche bouge plus que le lointain | 5 → 12 |
| `duration` | période d'un cycle | 6 → 11 s |
| `delay` | décalage de phase | ~1.4 s × index |

```json
{ "name": "float_3d", "loop": true, "duration": 7.3,
  "delay": 1.37, "amplitude": 9.0 }
```

## Choisir les périodes

Les périodes doivent être **non harmoniques**, sinon le groupe se resynchronise
au bout de quelques cycles et la danse revient. `7.3 / 9.1 / 6.1 / 8.3 / 10.7`
n'ont pas de petit multiple commun. Éviter `6 / 8 / 10`, qui se recalent toutes
les 120 s — mais surtout éviter d'utiliser la même période partout.

## Amplitude : léger

Au-delà de ~12 px le mouvement se regarde au lieu de se ressentir, et il entre
en concurrence avec l'animation d'entrée et le pan caméra. La caméra fournit
déjà le mouvement principal ; la parallaxe ne fait qu'écarter les plans.

## Vérifier plutôt que juger à l'œil

L'unisson est difficile à voir sur une lecture et évident sur une mesure. Suivre
le centroïde vertical de deux cards sur une fenêtre stabilisée et corréler les
deux séries : **+1.00 = unisson**, à corriger. En dessous de ~0.6 la divergence
est acquise.

## Piège historique

`float_3d` ignorait `delay` et `duration` : ses keyframes étaient figées à
0.0 / 0.5 / 1.0 s, donc tout élément flottant partageait un cycle d'une seconde,
en phase, quoi que demande le scénario. Les scénarios écrits avant ce correctif
passaient une `duration` sans effet — les relire plutôt que supposer qu'ils
appliquent déjà la règle.
