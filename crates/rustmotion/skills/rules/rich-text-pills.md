# Rule: `rich_text` — spans en pilule (fond, padding, radius, rotation)

Plusieurs références mettent en valeur un mot au milieu d'une phrase avec une petite étiquette arrondie, parfois légèrement tournée. Un span de `rich_text` peut porter sa propre boîte peinte derrière ses glyphes — l'étiquette suit alors le wrap et la ligne de base de la phrase, plutôt que d'être un `div` posé à côté qui casse dès que le texte change.

```json
{ "type": "rich_text", "style": { "font-size": 64 }, "spans": [
  { "text": "Automatisez vos " },
  { "text": "relances", "color": "#FFFFFF",
    "background": "#1F6FEB", "padding": { "top": 4, "right": 16, "bottom": 6, "left": 16 },
    "border-radius": 999, "rotation": -3 },
  { "text": " en un clic." } ] }
```

## Champs

| Champ | Rôle |
|---|---|
| `background` | Couleur (hex) du fond peint derrière le run de glyphes du span. Absent = pas de boîte ; `padding`, `border-radius` et `rotation` sont alors inertes, comme `text-autofit` sur un composant qui ne l'implémente pas. |
| `padding` | `{ top, right, bottom, left }` en px, chaque champ optionnel (défaut `0`). |
| `border-radius` | Rayon des coins de la boîte, en px. |
| `rotation` | Degrés, autour du **centre de la boîte**. N'affecte jamais la mise en page. |

## Le padding horizontal grossit l'avance de la ligne

`padding.left` et `padding.right` s'ajoutent à la largeur occupée par le span dans le flux — pas seulement à ce qui est peint. Sans ça, le span suivant recouvrirait la pilule au lieu de se décaler après elle. `padding.top`/`padding.bottom` ne touchent en revanche jamais `line-height` : ils grossissent la boîte verticalement, jamais l'interligne — deux pilules de tailles de police différentes sur la même ligne ne poussent donc pas les lignes voisines.

**GOOD** — le texte qui suit se décale après la pilule, il ne la recouvre pas :
```json
{ "type": "rich_text", "spans": [
  { "text": "tag", "background": "#1F6FEB", "padding": { "left": 16, "right": 16 } },
  { "text": " suite" } ] }
```

## Un span qui wrap redonne son padding à chaque fragment (`box-decoration-break: clone`)

Si le span-pilule contient plusieurs mots et que la ligne wrap au milieu, chaque fragment — un par ligne — reçoit sa **propre** boîte avec son propre padding gauche/droite, exactement le modèle CSS `box-decoration-break: clone`. Ce n'est pas une seule boîte étirée entre deux lignes.

Le point de wrap lui-même reste décidé sur la largeur brute du texte, sans compter le padding : un padding généreux sur une pilule proche de la largeur du conteneur peut donc légèrement déborder plutôt que de déclencher un retour à la ligne plus tôt qu'un span normal de même texte.

## `rotation` tourne la boîte et le texte ensemble, jamais la mise en page

`rotation` fait pivoter la pilule (fond + glyphes) comme un seul bloc rigide autour du centre de sa propre boîte. La position et la largeur que ce span réserve dans le flux du texte restent celles calculées sans rotation — un span à `rotation: -3` ne pousse pas ses voisins différemment d'un span identique à `rotation: 0`. C'est une transformation de peinture, pas de layout.

## Le fond est peint derrière les glyphes, jamais devant

La boîte est peinte en premier, puis les glyphes du run par-dessus — même ordre que `text-background` sur `text` (voir [rules/text-background.md](text-background.md)). Un span-pilule avec un `color` clair sur un `background` sombre reste donc lisible.

`text-background` couvre tout le composant `text` ; le fond de pilule ici ne couvre que le run d'un span à l'intérieur d'un `rich_text` — c'est la brique à choisir quand un seul mot au milieu d'une phrase doit être mis en évidence, pas toute la ligne. Voir aussi [rules/text-component-parity.md](text-component-parity.md) pour ce qui aligne déjà `text` et `rich_text` (espaces, ligne de base).
