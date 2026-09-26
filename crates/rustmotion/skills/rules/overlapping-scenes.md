# Scènes superposées : faire durer un élément à travers les coupes

Dans une vue `slide`, chaque scène est rendue seule et la `transition` mélange
deux images déjà finies : **rien ne survit à la coupe**. C'est ce qui fait lire une
vidéo comme une suite de diapositives plutôt que comme un plan continu.

En `timing: "v2"`, deux scènes dont les fenêtres se recouvrent ne se remplacent
plus — elles se **composent**. Une maquette qui monte au beat 3 et reste à l'écran
pendant que six libellés se succèdent par-dessus s'écrit comme une scène longue et
six scènes courtes :

```json
"timing": "v2",
"bpm": 120,
"composition": [{ "type": "slide", "scenes": [
  { "duration": 12.0, "children": [ … le décor qui tient les 12 s … ] },
  { "duration": 3.0, "at": "@0b",  "children": [ … beat 1 … ] },
  { "duration": 3.0, "at": "@6b",  "children": [ … beat 2 … ] },
  { "duration": 3.0, "at": "@12b", "children": [ … beat 3 … ] }
]}]
```

Durée totale : `max(at + duration)`, soit 12 s — pas 21 s. Chaque scène garde
**son propre temps** : une scène qui commence à `@6b` voit son `t` repartir de 0
quand sa fenêtre s'ouvre, donc ses animations d'entrée jouent à son arrivée et non
au début de la vidéo.

## L'ordre de composition, et pourquoi il décide du fond

Les participantes d'une frame sont empilées **dans l'ordre de déclaration** : la
première du tableau est en bas. Et c'est elle seule qui fournit l'arrière-plan —
les scènes au-dessus n'apportent que leurs enfants, sur un fond transparent.

Conséquence à connaître : `background` et `animated-background` déclarés sur une
scène qui n'est pas la plus basse de son recouvrement **ne peignent rien**. Le
décor appartient à la scène qui porte, pas à celles qui passent. C'est aussi ce qui
permet au fond de se transformer en continu sous les beats, au lieu d'être recoupé
à chaque cut.

Les `effects` de scène (grain, vignette, pixelate) suivent la même règle : ceux de
la scène du bas s'appliquent à l'image composée.

## Trois pièges

**Une scène ne peut pas à la fois se superposer et déclarer une `transition`.** Une
transition compose deux tampons de pixels finis ; un recouvrement compose des
scènes vivantes. Les deux ne peuvent pas décrire les mêmes frames. Rustmotion
avertit sur stderr et ignore la transition — retire-la, ou décale `at` pour que les
fenêtres ne se touchent plus.

**`snap: "beat"` ne crée jamais de recouvrement.** Arrondir une coupe sur la grille
peut la tirer avant la fin de la scène précédente ; ce n'est pas une demande de
composition, et le départ est repoussé (avec un avertissement). Un recouvrement se
déclare dans `at`, à la main. Sans cette distinction, `migrate` + `snap`
raccourcirait silencieusement tout fichier qu'il touche.

**Un trou gèle la dernière image.** Si aucune scène n'est vivante à un instant, la
dernière fenêtre fermée tient sa dernière frame — le rendu ne devient jamais noir
par accident.

## Quand préférer une vue `world`

Le recouvrement fait durer un élément **au même endroit du cadre**. La vue `world`
fait autre chose : une caméra traverse un espace où chaque scène occupe une
position. Prends `world` pour un travelling, le recouvrement pour un décor qui
tient pendant que le contenu change. Voir [world-view.md](world-view.md).
