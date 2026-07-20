# Style "1600" — motion brutalist à blocs de couleur

Recette pour produire des vidéos dans l'esprit des studios de motion design type [1600.agency](https://www.1600.agency/) : typographie massive, blocs de couleur saturés plein cadre, kinetic typography, transitions franches. Exemple de référence : `examples/1600-style.json`.

## Langage visuel

- **Typographie** : une seule grotesque condensée ultra-bold en `UPPERCASE`, partout. Anton (Google Font) est le choix canonique. Déclarer au niveau racine :
  ```json
  "fonts": [{ "family": "Anton", "source": "google", "weights": [400] }]
  ```
  puis `"font-family": "Anton"` sur chaque texte. `line-height` serré (0.92–1.02) pour empiler les lignes, `letter-spacing` 1–2 sur les gros titres, 4–8 sur les petits kickers.
- **Tailles (16:9, 1920×1080)** : hero 240–300px, titres 120–200px, chiffres 190px, kicker/label 32–56px. On cherche le texte qui remplit la largeur.
- **Palette — blocs saturés alternés** : chaque scène est un aplat plein cadre qui bascule brutalement. Un seul aplat par scène, texte en noir `#0A0A0A` ou crème `#F5F0E8` selon le fond.
  - Jaune `#FFE500`, cobalt `#1A1AE5`, corail `#FF4A32`, vert acide `#00E676`, noir `#0A0A0A`.
  - Accent sur un mot : réutiliser une des couleurs de bloc (ex. jaune sur fond noir).
- **Fond de scène** : un `shape` `rounded_rect` (border-radius 0) plein cadre `1920×1080` en `position: absolute` posé en **premier** enfant ; le contenu passe en flow centré au-dessus.

## Animation

- **Kinetic entrances** : `slide_in_up` (lignes empilées, stagger 0.12s), `slide_in_left` (listes de services), `scale_in` avec `overshoot` (mots accent, CTA). Jamais de fondu mou.
- **Transitions entre scènes** : franches et directionnelles — `wipe_up` / `wipe_left` / `wipe_right` / `wipe_down` / `slide`, durée 0.35s. Alterner les directions pour le rythme.
- **Chiffres** : `counter` (`from`/`to`, `prefix`), il monte sur la durée de la scène. Réserver sa hauteur (`"height"` ≈ font-size + 10) et un `gap` ≥ 24 avec son label, sinon le label remonte sur le chiffre (le counter hors `card` n'a pas de correction de baseline).

## Profondeur & caméra — jouer sur l'espace

Sans ça, le style est un diaporama 2D. Trois leviers, cumulés sur chaque scène :

1. **Caméra en mouvement continu** (`scene.camera.keyframes`, propriétés `zoom` / `origin.x` / `origin.y` / `rotation`) : un push-in ou pull-out lent (zoom 1.0↔1.14) + une dérive du point focal (`origin`) donne une vie permanente. Alterner push-in / pull-out d'une scène à l'autre.
2. **Plans de profondeur** (`style.depth` sur les enfants **directs** de la scène, qui deviennent les plans de parallaxe) : structurer chaque scène en 3 plans absolus plein cadre —
   - **fond profond** `depth ~0.4` : un mot/chiffre géant ton-sur-ton (`overflow: hidden` sur le plan pour le clipper au cadre) ;
   - **contenu** `depth 1.0` : la typo principale ;
   - **avant-plan** `depth ~1.75` : petites formes d'accent avec `float_3d` en boucle.
   Au mouvement caméra, les plans se séparent → vraie profondeur.
3. **Entrées en rotation 3D** sur la ligne clé : `flip_in_x` / `flip_in_y` / `tilt_in` (+ `perspective` 900–1400 sur l'élément, `transform-origin` pour pivoter sur une charnière), qui se résolvent face caméra → lisible une fois posé.

**Piège du bleed** : une `rotation` caméra (ou un zoom < 1.0) révèle le `video.background` aux coins. Garder un zoom couvrant pendant toute la rotation (`zoom ≥ ~1.06` pour ±2°), ou fixer `video.background` à la couleur de la scène. Un `origin` qui dérive à zoom 1.0 ne bleede pas (l'origin n'a d'effet qu'avec du zoom).

## Structure d'une scène type

```json
{
  "duration": 3.4,
  "transition": { "type": "wipe_up", "duration": 0.35 },
  "layout": { "direction": "column", "align_items": "center", "justify_content": "center", "gap": 6 },
  "children": [
    { "type": "shape", "shape": "rounded_rect", "fill": "#0A0A0A", "position": "absolute", "x": 0, "y": 0, "style": { "width": 1920, "height": 1080, "border-radius": 0 } },
    { "type": "text", "content": "STUDIO DE", "style": { "font-family": "Anton", "font-size": 150, "color": "#F5F0E8", "line-height": 0.95, "animation": [{ "name": "slide_in_up", "duration": 0.55 }] } },
    { "type": "text", "content": "MOTION DESIGN", "style": { "font-family": "Anton", "font-size": 200, "color": "#FFE500", "line-height": 0.95, "animation": [{ "name": "scale_in", "delay": 0.18, "duration": 0.6, "overshoot": 0.12 }] } }
  ]
}
```

## Pour dupliquer sur un autre contenu

Garder le squelette (aplat + typo Anton + entrées kinetic + transitions franches), remplacer les textes/chiffres/palette. Rythme : 3–4s par scène, une idée par scène. Réutiliser une couleur d'accent cohérente d'un bout à l'autre.
