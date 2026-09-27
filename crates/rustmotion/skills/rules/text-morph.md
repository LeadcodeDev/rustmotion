# Rule: `text.morph` — une transition lettre par lettre entre deux labels

`text.states` + `swap` (voir [rules/text-polish.md](text-polish.md)) traite chaque label comme un bloc rigide : l'ancien monte en se floutant, le nouveau monte du bas en se nettant. `morph` traite le label comme un sac de glyphes qui se réarrange — les caractères identiques glissent de leur ancienne position vers leur nouvelle, les autres apparaissent ou disparaissent sur place.

```json
{ "type": "text", "content": "On sait très bien que",
  "states": [{ "at": 1.2, "content": "tu les griffonnes" }],
  "morph": { "duration": 0.6, "unmatched": "scramble", "seed": 2 },
  "style": { "font-size": 64, "white-space": "nowrap" } }
```

C'est un champ **séparé** de `swap`, pas un mode de celui-ci — poser les deux en même temps n'a pas de sens : `morph` prend la fenêtre de transition dès qu'il s'applique, `swap` n'est consulté qu'en dehors.

## L'appariement : plus long sous-motif commun, de gauche à droite

Les caractères identiques entre le label sortant et le label entrant sont appariés par plus longue sous-séquence commune (LCS) — le même algorithme qu'un diff de texte. Un caractère apparié **glisse** de sa position dans l'ancien label vers sa position dans le nouveau ; il reste à pleine opacité pendant tout le trajet, seule sa position interpole. Un caractère qui n'a pas de partenaire (présent dans un seul des deux labels) ne glisse jamais : il s'estompe sur place.

```json
{ "type": "text", "content": "AX", "states": [{ "at": 1.0, "content": "YA" }],
  "morph": { "duration": 1.0 } }
```

Ici le seul `A` commun glisse de sa position dans `"AX"` vers sa position dans `"YA"` ; le `X` sortant s'efface sur place, le `Y` entrant apparaît sur place — trois trajectoires indépendantes, pas un bloc qui se translate.

## `unmatched` : comment les glyphes sans partenaire arrivent

| Valeur | Comportement |
|---|---|
| `fade` (défaut) | Le glyphe entrant apparaît directement en fondu — c'est le glyphe final du premier au dernier instant, seule son opacité change. |
| `scramble` | Le glyphe entrant cycle à travers des caractères aléatoires (déterministes, dérivés de `seed`) pendant la première partie de la transition, puis se fixe sur le vrai glyphe pour la finir. |

`scramble` ne s'applique qu'aux glyphes **entrants** sans partenaire : un glyphe sortant n'a pas de "caractère final" vers lequel se fixer, il s'estompe donc toujours comme en mode `fade`.

`seed` ne pilote que la séquence de caractères de remplacement — deux morphs avec le même `seed` scramblent identiquement ; il n'a aucun effet en mode `fade`.

## Ce que `morph` ne fait pas

Il ignore `visible_chars_progress` (un `typewriter` en cours) exactement comme `swap` — les deux mécanismes agissent sur le label complet, pas sur une révélation progressive en cours. Il ne wrap pas différemment de la mise en page normale du composant : chaque label est réenveloppé (`wrap_text_with_tracking`) séparément avec le même `style`, donc un caractère apparié peut glisser en diagonale s'il change de ligne entre les deux labels — un mouvement plausible, pas un artefact à corriger.
