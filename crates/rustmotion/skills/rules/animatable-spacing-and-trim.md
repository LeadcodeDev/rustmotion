# Deux propriétés qui manquaient : `letter_spacing` et `draw_start`

Toutes deux s'animent par `keyframes`, comme `opacity` ou `scale`.

```json
"style": { "animation": [{ "name": "keyframes", "keyframes": [
  { "property": "letter_spacing",
    "keyframes": [{ "time": 0, "value": 0 }, { "time": 0.8, "value": 18 }],
    "easing": "ease_out" }
]}]}
```

## `letter_spacing` : le texte déborde, et c'est voulu

La mesure intrinsèque du nœud garde la valeur **statique** de
`style.letter-spacing`. Animer l'interlettrage ne relance donc pas le layout —
c'est la même politique que partout ailleurs : interpoler une propriété de layout
demanderait de recalculer les boîtes à chaque frame échantillonnée.

Conséquence : un texte dont l'interlettrage s'ouvre **dépasse de sa boîte**.
Dimensionne la boîte sur la valeur finale, ou accepte le débordement en connaissance
de cause.

Attention au double mécanisme : un `timeline` qui change `letter-spacing` **saute**
et le validateur t'avertit ; une piste `keyframes` sur `letter_spacing`
**interpole**. Ce ne sont pas les mêmes noms par hasard — l'un est une propriété
CSS dans un état, l'autre une propriété de mouvement.

## `draw_start` : l'autre bout du tracé

`draw_progress` avance la tête du tracé. `draw_start` avance sa **queue**. Les deux
ensemble définissent une fenêtre qui se déplace le long du chemin — un trait qui
court, plutôt qu'un trait qui pousse.

`draw_start` seul, sans `draw_progress`, efface le début d'un tracé complet.

Fonctionne sur `line` et `svg`. Sur `shape`, c'est le champ de composant
`shape.draw_start` qu'il faut — voir
[shape-draw-start-and-path-morph.md](shape-draw-start-and-path-morph.md) — parce
que `shape` porte aussi `path_morph` et que les deux vont ensemble.

**Le découpage passe par `PathMeasure`, pas par un effet de tirets.** C'est ce qui
évite le point parasite qu'un tiret de longueur nulle avec un capuchon rond
produisait (#376). En revanche le capuchon déborde toujours d'une demi-épaisseur
en arrière du point de coupe : c'est le capuchon qui fait son travail, pas le
découpage qui rate.
