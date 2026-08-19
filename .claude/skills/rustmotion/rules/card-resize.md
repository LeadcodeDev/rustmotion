# Rule: Redimensionner une carte (et pas la mettre à l'échelle)

Une carte compacte qui grandit en panneau de détail est un changement de **layout** : sa nouvelle boîte reflue son contenu. Un `scale` étire les pixels qu'elle avait déjà, texte compris — c'est un zoom, pas un redimensionnement, et ça se voit immédiatement sur le texte devenu flou et surdimensionné.

Anime `width` / `height` avec un effet `keyframes` : ils atteignent taffy, donc la mise en page est recalculée à chaque frame.

```json
{
  "type": "card",
  "style": {
    "width": "330px",
    "height": "132px",
    "background": "#111C33",
    "border-radius": 20,
    "justify-content": "center",
    "align-items": "center",
    "animation": [{
      "name": "keyframes",
      "delay": 1.2,
      "duration": 0.9,
      "keyframes": [
        { "property": "width",  "easing": "ease_out_cubic",
          "keyframes": [{ "time": 0.0, "value": 330 }, { "time": 0.9, "value": 620 }] },
        { "property": "height", "easing": "ease_out_cubic",
          "keyframes": [{ "time": 0.0, "value": 132 }, { "time": 0.9, "value": 240 }] }
      ]
    }]
  },
  "children": [ { "type": "text", "content": "…" } ]
}
```

## Les temps des keyframes sont relatifs au `delay`

`{"time": 0.0}` est le début de l'effet, pas le début de la scène. Le `delay` de l'effet décale toute la piste.

## La taille animée gagne sur la taille intrinsèque

Un composant qui déclare sa propre taille (une `shape`, un `badge`) la voit remplacée pendant l'animation. C'est voulu, mais ça veut aussi dire qu'une valeur oubliée à 0 sur la dernière keyframe fait disparaître la boîte.

## Que le contenu suive vraiment

Sans `justify-content` / `align-items`, l'enfant reste collé en haut à gauche et seule la boîte grandit : le mouvement paraît vide. Centre le contenu (ou donne-lui un `flex: 1`) pour que la croissance se lise.

## Le validateur voit la boîte finale

`validate --strict-anim` échantillonne l'animation : une carte qui grandit hors du device est signalée à l'instant où ça arrive. Vérifie qu'il reste de la marge à la taille maximale, pas seulement à la taille initiale.
