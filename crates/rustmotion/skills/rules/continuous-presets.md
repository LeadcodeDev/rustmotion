# Rule: Continuous Presets Need loop: true

The presets `pulse`, `float`, `shake`, and `spin` are continuous animations. Without `"loop": true`, they play once and stop.

**GOOD:**
```json
{ "style": { "animation": [{ "name": "float", "loop": true }] } }
```

**BAD:**
```json
{ "style": { "animation": [{ "name": "float" }] } }
```

Continuous presets: `pulse`, `float`, `shake`, `spin`.

## `speed` + `direction` : seuls quatre fonds se laissent faire défiler

`direction` translate la texture du fond. Ça n'a de sens que pour un motif
**périodique sous translation**, qui n'a par ailleurs aucun mouvement propre :

| Preset | `direction` | Pourquoi |
|---|---|---|
| `grid_dots`, `grid_lines`, `pixel_grid`, `heropattern` | **actif** | Motifs pavés, dessinés avec une période entière de marge de chaque côté. Le défilement extérieur est leur seul mouvement. |
| `gradient_shift` | **inerte** | `speed` pilote déjà le sens de rotation du dégradé (`direction` vaut `cw`/`ccw` ici), et le shader est peint sur le rectangle du cadre **sans marge** : toute translation laissait une bande découverte. |
| `concentric_circles` | **inerte** | Calcule déjà son propre `offset = (time * speed) % spacing`. Translater un motif radial déplace son centre — c'était une double animation. |
| `halo` | **inerte** | Anime ses zones lui-même. |

Sur les trois derniers, déclarer une `direction` ne produit plus **rien du tout**
(vérifié frame par frame, pixel pour pixel). C'est un changement de rendu visible
pour un scénario existant qui en déclarait une — mais le mouvement supprimé est
celui qui faisait sortir le fond du cadre, pas un effet qu'on perd.

> `pixel_grid` a son propre champ `motion` (`twinkle`, `sweep`), qui ne translate
> rien : il compose avec le défilement au lieu de le doubler. Son défaut,
> `motion: none`, en fait une texture immobile — exactement le cas de `grid_dots`.
