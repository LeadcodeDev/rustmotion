# Rule: Les quatre finitions de texte

Quatre mécanismes qui demandaient chacun un sous-arbre bricolé à la main s'écrivent maintenant en une ligne. Aucun ne remplace un preset d'entrée : ils s'ajoutent par-dessus.

## `shimmer` — la lumière passe sur les lettres

Effet d'animation, pas champ de composant. La bande n'éclaire **que les pixels réellement peints** (composée en `SrcATop` dans la couche du nœud) : sur un `text`, la lumière accroche les glyphes, pas la boîte.

```json
"animation": [{
  "name": "shimmer",
  "delay": 1.0, "duration": 1.1,
  "color": "#7DD3FC", "intensity": 0.85,
  "width": 0.3, "angle": 22, "loop": true
}]
```

`width` est la largeur de la bande en fraction du trajet (0.3 = glint net, 0.8 = lavage doux). `angle` incline la bande : `0` est verticale et balaie de gauche à droite ; ~20° est ce qui la fait lire comme un reflet plutôt que comme un essuyage. La couche isolée n'est ouverte que pendant la fenêtre de l'effet — un `shimmer` non bouclé ne coûte rien le reste de la scène.

Combiné à `char_blur_in` sur le même `text`, ça reproduit le « text stagger » : les mots montent en se défloutant, puis la lumière passe.

## `text.states` — un libellé qui en devient un autre

```json
{
  "type": "text",
  "content": "Saving draft",
  "states": [{ "at": 2.6, "content": "Saved" }],
  "swap": { "duration": 0.45, "distance": 22, "blur": 9 },
  "style": { "font-size": 52, "white-space": "nowrap", "max-width": "600px" }
}
```

Sans `swap`, les libellés se coupent net à chaque `at` — abrupt, mais c'est exactement ce que demande l'absence du champ. Avec `swap`, les deux sont à l'écran pendant la fenêtre : le sortant monte en floutant, l'entrant monte du bas en se défloutant.

**La boîte est mesurée sur le libellé le plus long**, pas sur le premier. Une boîte dimensionnée pour `"Saved"` déborderait à l'instant du retour vers `"Saving draft"` — et le validateur aurait signé.

## `text.caret` — le caret suit la révélation

```json
{ "type": "text", "content": "rustmotion --frames 0-60",
  "caret": { "shape": "block", "blink": 0.9, "color": "#38BDF8" },
  "style": { "animation": [{ "name": "typewriter", "duration": 2.0 }] } }
```

`shape`: `line` (règle fine) ou `block` (terminal). `blink` est la période complète en secondes (`0` = fixe). `hide_when_done: true` retire le caret une fois la révélation finie au lieu de le laisser garé.

C'est la raison d'être du champ : un `cursor` composé à côté du texte reste où on l'a mis pendant que le texte pousse sous lui. Le caret est aussi présent **avant** le premier caractère, sinon la première frame est vide puis caret et lettre apparaissent ensemble, ce qui lit comme un glitch.

## `pop_in` — l'arrivée de badge

Preset d'animation : l'élément grossit depuis rien avec un dépassement `back-out`, **puis** une courte impulsion élastique une fois posé. `overshoot` règle l'amplitude de l'impulsion (défaut 0.18 = 118 %) ; `0` la supprime et laisse un simple scale-in.

Les deux temps comptent : le premier place l'élément, le second y ramène l'œil. Fondus en une seule courbe, ils lisent comme un tremblement.
