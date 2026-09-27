# Rule: les `char_*` fonctionnent aussi sur `rich_text` et `gradient_text`

Les sept presets `char_*` (`char_scale_in`, `char_fade_in`, `char_wave`, `char_bounce`, `char_rotate_in`, `char_slide_up`, `char_blur_in`) ne sont plus réservés à `text`. Posés dans `style.animation` de `rich_text` ou de `gradient_text`, ils animent désormais réellement les glyphes — avant ce correctif, ils étaient acceptés sans erreur et **n'avaient aucun effet** : le composant peignait son texte complet, net, dès la première frame. Voir [rules/char-animation-tuning.md](char-animation-tuning.md) pour le réglage commun (`direction`, `distance`, `scale_from`, `jitter`+`seed`, `ink_from`, `blur`) — il s'applique tel quel aux trois composants.

```json
{ "type": "rich_text", "spans": [
    { "text": "Rich ", "color": "#1B1B25" },
    { "text": "words", "color": "#1D5FD8" }
  ], "style": { "font-size": 64,
    "animation": [{ "name": "char_fade_in", "stagger": 0.05, "duration": 0.3, "delay": 0.5 }] } }
```

## `rich_text` : l'unité court à travers toute la phrase, pas span par span

En mode `"granularity": "word"`, l'indexation d'unité (donc le `stagger`) traverse **tous** les spans dans l'ordre de lecture, pas seulement le span courant. Un `rich_text` de deux spans formant "ONE TWO" avec `stagger: 0.6` retarde le second mot exactement comme un `text` unique "ONE TWO" l'aurait fait — découper la phrase en spans pour la colorer ne change pas le calendrier de l'animation.

En mode `"char"`, les espaces entre mots ne consomment pas de créneau d'unité (ils ne sont de toute façon jamais dessinés comme glyphe séparé dans `rich_text` — leur largeur est déjà absorbée dans le positionnement du mot suivant).

## `rich_text` : `ink_from` converge vers la couleur **propre** du span

Chaque unité anime avec la police et la couleur de **son** span. `ink_from` (couleur de départ) converge donc vers la couleur finale de ce span précis, pas vers une couleur par défaut partagée :

```json
{ "type": "rich_text", "spans": [
    { "text": "AAAA", "color": "#00FF00" },
    { "text": " BBBB", "color": "#0000FF" }
  ], "style": {
    "animation": [{ "name": "char_fade_in", "granularity": "word", "ink_from": "#FF0000" }] } }
```

Les deux mots partent du même rouge et convergent chacun vers sa propre couleur (vert, puis bleu) — pas vers une troisième couleur commune.

Les fonds de pilule (`background` sur un span, voir [rules/rich-text-pills.md](rich-text-pills.md)) restent peints normalement, sans animation, pendant qu'un `char_*` anime les glyphes par-dessus.

## `gradient_text` : le dégradé reste celui de la phrase entière

Découper le texte en mots ou en caractères animés **ne relance jamais la rampe** pour chaque unité. Le dégradé est calculé une seule fois sur l'étendue du texte complet, avant tout découpage ; chaque unité se contente de peindre sa portion de glyphes avec ce même dégradé, à sa position réelle. Un texte "IIII IIII IIII IIII" avec `char_fade_in` en mode mot garde donc son premier mot proche de la première couleur et son dernier mot proche de la dernière, exactement comme sans animation.

```json
{ "type": "gradient_text", "content": "Build it faster today",
  "colors": ["#FF4FB0", "#4B5BFF"],
  "style": { "font-size": 64,
    "animation": [{ "name": "char_blur_in", "granularity": "word", "stagger": 0.08 }] } }
```

## `gradient_text` : `ink_from` n'a pas d'effet visible

`ink_from` fonctionne en teintant le `Paint` du texte — sur `text` et `rich_text`, ce `Paint` porte une couleur unie, donc la teinte se voit. Sur `gradient_text`, le `Paint` porte un **shader** (le dégradé) : le shader l'emporte toujours sur la composante couleur du `Paint`, donc `ink_from` ne change rien au rendu (seule son influence sur l'alpha survit, imperceptible). Ce n'est pas un oubli à corriger silencieusement en aval : c'est une conséquence directe du fait que la teinte d'un dégradé n'a pas de valeur "de départ" unique et cohérente à faire converger. Les six autres réglages (position, échelle, rotation, flou, alpha) fonctionnent normalement.

## Ce qui n'a pas bougé

`rotate_from`, le bruit indépendant par unité (`scale_jitter`/`baseline_jitter` distincts du décalage de `stagger`), et le reflow animé (`reflow`) demandés par ailleurs restent absents des trois composants — `jitter`+`seed` continuent de ne régler que l'irrégularité du **calendrier** de départ, pas la géométrie de chaque unité. Aucun de ces noms n'existe dans le schéma : les déclarer serait accepté puis ignoré, exactement le défaut que ce correctif referme pour les sept presets eux-mêmes.
