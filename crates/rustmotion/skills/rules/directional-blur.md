# Rule: Flou directionnel — `radius-x`/`radius-y`, `directional-blur`, `blur_x`/`blur_y`

`filter: [{ "fn": "blur", "radius": N }]` est isotrope : à forte valeur, un mot qui traverse le cadre bave verticalement autant qu'horizontalement et perd sa forme. Trois briques couvrent le cas d'un élément étiré le long de son axe de déplacement — un aplat qui glisse, un mot qui whip, une étiquette qui tombe.

## Flou statique par axe : `radius-x` / `radius-y`

```json
{ "filter": [{ "fn": "blur", "radius-x": 40, "radius-y": 0 }] }
```

`Blur` a maintenant trois champs, tous optionnels : `radius` (isotrope, comportement historique), `radius-x`, `radius-y`. Quand `radius-x`/`radius-y` sont donnés ils l'emportent sur `radius` **sur leur propre axe** ; l'axe omis retombe sur `radius` si présent, sinon vaut 0. `{ "radius-x": 40, "radius-y": 0 }` donne un flou strictement horizontal ; `{ "radius": 24 }` reste un flou isotrope classique — aucun scénario existant ne change de rendu.

> Piège de casing : ces deux champs sont en kebab-case (`radius-x`), comme tout le reste de `FilterFn` — cohérent avec `offset-x`/`offset-y` de `box-shadow` et `drop-shadow`. `blur_x`/`blur_y` (voir plus bas) sont en revanche en **snake_case**, parce qu'ils vivent dans l'espace de noms des propriétés animables (`translate_x`, `scale.x`, …), pas dans celui des champs de filtre — deux conventions différentes, chacune cohérente avec ses voisines immédiates.

## Flou statique en diagonale : `directional-blur`

```json
{ "filter": [{ "fn": "directional-blur", "angle": 90, "radius": 40 }] }
```

Pour un déplacement qui n'est ni horizontal ni vertical. `angle` est en degrés (`0` = along +x, `90` = along +y), `radius` la longueur du flou le long de cet axe. Rendu par rotation du contenu échantillonné autour du centre de la boîte, flou mono-axe, rotation inverse — pas une vraie convolution orientée, mais visuellement équivalent pour un flou raisonnable, et sans coût suffisant pour justifier un noyau dédié.

## Flou animé : `blur_x` / `blur_y`

```json
{ "property": "blur_x", "keyframes": [{ "time": 0, "value": 40 }, { "time": 0.3, "value": 0 }] }
```

Deux nouvelles propriétés animables (`KNOWN_MOTION_PROPERTIES`), au même titre que `blur` (qui reste isotrope et inchangé). Elles se posent en filtre `Blur { radius-x, radius-y }` sur le nœud — se combinent avec un `radius`/`radius-x`/`radius-y` statique déclaré par ailleurs en s'ajoutant à la liste `filter`, pas en le remplaçant.

## `motion_blur mode: "smear"` en tire parti automatiquement

Voir [rules/motion-blur-and-trail.md](motion-blur-and-trail.md) — le mode `smear` de `motion_blur` calcule lui-même un `radius-x`/`radius-y` à partir de la vitesse instantanée et pose ce même filtre `Blur`, sans que l'auteur du scénario ait à l'écrire à la main.

## Ce qui n'est pas câblé : `blur_axis` par caractère

L'issue #360 proposait aussi un axe de flou **par unité** sur les presets `char_*` :

```json
{ "name": "char_blur_in", "granularity": "word", "blur_axis": "motion", "stretch": 1.3 }
```

Non implémenté : le rendu par-caractère (`char_*`) vit dans le composant `text` et son renderer, pas dans `animator.rs`/`box_builder.rs`. Ajouter `blur_axis`/`stretch` au schéma sans que le renderer les consomme produirait un champ accepté mais inerte — exactement ce que ce projet évite ailleurs (`text-autofit` sur un composant qui ne l'implémente pas). À traiter comme un chantier séparé, dans `text.rs`/`renderer/text.rs`.
