# Rule: Correspondance Hyperframes → rustmotion

Si on te demande un effet du catalogue Hyperframes (ou un effet décrit dans ce vocabulaire — «streaming text», «number wheel», «badge pop»…), **cherche-le dans cette table avant d'écrire quoi que ce soit**. La moitié de ces effets existe déjà sous un autre nom, et la reconstruire à la main donne un résultat moins bon et non vérifiable par le validateur.

| Hyperframes | En rustmotion |
|---|---|
| Blur In | `style.animation: [{ "name": "char_blur_in", "granularity": "word" }]` |
| Staggered Fade Up | `char_blur_in` / `char_slide_up` + `direction`, `distance`, `scale_from` |
| Top Down Letters | `char_slide_up` avec `"direction": "down"` |
| Text Stagger | `char_blur_in` (montée) + effet `shimmer` (balayage) sur le même `text` |
| Number Pop In | `char_blur_in` avec `"granularity": "char"`, `"scale_from": 0.82` |
| Streaming Text | `char_blur_in` avec `jitter`/`seed`/`ink_from` — voir [streaming-text.md](streaming-text.md) |
| Typewriter | preset `typewriter` + `text.caret` |
| Text State Swap | `text.states` + `text.swap` |
| Number Wheel | composant `number_wheel` — voir [number-wheel.md](number-wheel.md) |
| Badge Pop | `badge` + `style.animation: [{ "name": "pop_in" }]` |
| Success Check | composant `success_check` |
| Simulated Cursor | composant `pointer` — voir [pointer-walkthrough.md](pointer-walkthrough.md) |
| Card Resize | `keyframes` sur `width`/`height` — voir [card-resize.md](card-resize.md) |
| Arc Motion Path | effet `motion_path` + `orient` — voir [motion-path.md](motion-path.md) |
| SVG Line Draw Loader | preset `draw_in` / `stroke_reveal` sur un `svg` |
| Dynamic Grid | `animated-background` preset `grid_lines` |
| Page Slide | `transition: { "type": "slide" }` |
| Chromatic Aberration Wipe | `transition: { "type": "chromatic_wipe" }` |

## Deux pièges de nommage

`cursor` **n'est pas** un curseur de souris : c'est un caret texte (une barre clignotante). Le pointeur de souris avec son anneau de clic, c'est `pointer`.

`counter` **n'est pas** une roue de chiffres : il interpole une *valeur* et réécrit le nombre à chaque frame, donc les glyphes sautent. `number_wheel` fait défiler des bandes de chiffres, comme un compteur mécanique. Un compteur qui monte de 0 à 30 222 → `counter`. Un chiffre qui atterrit → `number_wheel`.
