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
