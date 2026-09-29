# Rule: text, rich_text et gradient_text alignés entre eux

Trois composants texte qui divergaient silencieusement l'un de l'autre — même police, même taille, rendu différent. Les trois écarts ci-dessous sont corrigés ; ce qui suit documente le modèle correct pour générer avec.

## text : l'alpha de style.color est respecté

`style.color` avec un canal alpha (`#RRGGBBAA`) est rendu tel quel sur `text` — l'alpha n'est plus forcé à `255` (opaque) après coup. `rich_text`, les fonds et les bordures ont toujours respecté cet alpha ; `text` était l'exception. Le contournement historique (couleur opaque + `style.opacity` sur le nœud) n'est plus nécessaire :

**GOOD:**
```json
{ "type": "text", "content": "echo", "style": { "font-size": 120, "color": "#FFFFFF12" } }
```

## rich_text : white-space: pre préserve les espaces, comme text

`white-space: "pre"` (ou `"nowrap"`) sur `rich_text` garde le texte de chaque span **littéral** — espaces de tête, espaces multiples internes — au lieu de le retokeniser en mots séparés par un espace unique. C'est le même contrat que sur `text`. Sans `white-space: pre`/`nowrap`, le comportement historique (mots wrappables, espaces source collapsés à un seul) est inchangé.

**GOOD** — indentation préservée pour un rendu façon terminal :
```json
{ "type": "rich_text", "spans": [{ "text": "    $ npm install", "color": "#E5E7EB" }],
  "style": { "font-size": 22, "white-space": "pre", "font-family": "monospace" } }
```

## rich_text : la ligne de base suit la même formule que text

`rich_text` calcule maintenant son décalage de ligne de base comme `text` : `(line_height + ascent - descent) / 2`. Avant, `descent` n'entrait pas dans le calcul et un `rich_text` rendait plusieurs pixels plus bas qu'un `text` à police et taille identiques (~9px à 72px) — un `text` remplacé par un `rich_text` entre deux scènes, ou les deux posés côte à côte sur la même ligne, sautait visiblement. Aucun champ à changer côté JSON : c'est un correctif de rendu interne.

## gradient_text : angle façon CSS (breaking change sur le défaut)

`angle` suit désormais la convention CSS des `linear-gradient` — la même que `view.background` en `linear-gradient` : `0` = vers le haut, `90` = vers la droite, `180` = vers le bas, `270` = vers la gauche. La valeur par défaut reste `90` (donc horizontal gauche→droite), mais **avant ce correctif le même nombre produisait un dégradé tourné de 90° par rapport à cette convention** (l'ancien défaut de `90` rendait un dégradé vertical). Un scénario qui fixait `angle` explicitement pour compenser (typiquement `angle: 0` pour obtenir de l'horizontal) doit être revu : sous la nouvelle convention, `angle: 0` est désormais vertical.

La ligne du dégradé est aussi recalculée : c'est la projection CSS de la boîte de texte sur l'axe choisi (`|w·sin θ| + |h·cos θ|`), plus la diagonale de la boîte. Avant, un texte large et court avec un dégradé vertical n'utilisait qu'une fraction centrale de la rampe (les glyphes lisaient une teinte quasi plate) ; désormais la première couleur touche le premier bord de glyphe et la dernière couleur le dernier bord, à tout angle.

## gradient_text : stops explicites

Champ optionnel `stops`. Il ne partage pas la clé des stops d'un fond : `gradient_text` nomme la position `position`, un `background` la nomme `offset`. Les deux sont une fraction `0..1` de la ligne du dégradé — seul le nom diffère, et le validateur refuse l'autre.

```json
{ "type": "gradient_text", "content": "Rustmotion", "angle": 90,
  "stops": [
    { "color": "#7C3AED", "position": 0 },
    { "color": "#EC4899", "position": 0.7 },
    { "color": "#F59E0B", "position": 1 }
  ] }
```

Sans `stops`, `colors` reste réparti uniformément sur la ligne — comportement historique inchangé, `colors` seul continue de fonctionner tel quel.
