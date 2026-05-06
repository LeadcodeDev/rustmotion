# Rule: HTML/CSS Mental Model

## Principe

L'API JSON de Rustmotion est un superset de HTML/CSS. Chaque scène est un flex container, chaque `card` est une `<div>`, les propriétés CSS (`gap`, `padding`, `flex-direction`, `align-items`, etc.) ont exactement la même sémantique.

**Avant d'écrire du JSON, se poser cette question : "Comment j'écrirais ça en HTML/CSS ?"**

---

## Correspondances directes

| HTML/CSS | Rustmotion JSON |
|---|---|
| `<body>` avec `display:flex; flex-direction:column; align-items:center; justify-content:center` | `"layout": {"direction": "column", "align_items": "center", "justify_content": "center"}` |
| `<div style="display:flex; flex-direction:row; gap:24px">` | `{"type": "card", "style": {"flex-direction": "row", "gap": 24}}` |
| `<div style="display:grid; grid-template-columns:1fr 1fr; gap:16px">` | `{"type": "card", "style": {"display": "grid", "grid-template-columns": [{"fr":1},{"fr":1}], "gap": 16}}` |
| `<h1>Titre</h1>` — élément inline, sans position | `{"type": "text", "content": "Titre"}` — flow child, pas de `x`/`y` |
| `<p style="text-align:center; font-size:40px; color:#94A3B8">` | `{"type": "text", "style": {"text-align": "center", "font-size": 40, "color": "#94A3B8"}}` |
| `<svg>` décoratif en `position:absolute; top:0; left:0; z-index:-1` | `{"type": "shape", "position": "absolute", "x": 0, "y": 0, "style": {"z-index": 0}}` |
| `padding`, `margin`, `gap` | mêmes noms, mêmes effets |
| `border-radius`, `box-shadow`, `backdrop-filter` | mêmes noms |
| `opacity`, `overflow`, `z-index` | mêmes noms |

---

## Flow vs Absolute

### Normal flow (par défaut)

Les enfants sans `position` participent au flux flex/grid. Ils s'empilent naturellement et sont centrés par le layout du parent. **C'est la règle par défaut pour tout le contenu principal.**

```json
{
  "layout": { "direction": "column", "align_items": "center", "justify_content": "center", "gap": 40 },
  "children": [
    { "type": "icon", "icon": "lucide:zap", "style": { "width": 160, "height": 160, "color": "#6366F1" } },
    { "type": "text", "content": "Titre principal", "style": { "font-size": 96, "font-weight": "bold", "color": "#FFFFFF", "text-align": "center" } },
    { "type": "text", "content": "Sous-titre lisible", "style": { "font-size": 48, "color": "#94A3B8", "text-align": "center" } },
    { "type": "card", "style": { "width": 900, "padding": 40, "gap": 24, "flex-direction": "row" }, "children": [ ... ] }
  ]
}
```

### `position: "absolute"`

Retire l'élément du flux. Placé à des coordonnées `x`/`y` relatives à son parent (ou à la scène si enfant direct de la scène).

**Utiliser uniquement pour :**
- Blobs et shapes décoratifs en fond
- Layers de particules
- Badges ou tooltips flottants qui se superposent au contenu
- Éléments qui ne doivent PAS influencer le layout de leurs voisins

**Ne jamais utiliser pour :**
- Titres, sous-titres, corps de texte
- Cards principales, grilles, sections de contenu
- Icônes hero
- Tout ce qui fait partie de la "pile" visuelle de la scène

---

## Patterns courants

### Stack vertical centré (le plus fréquent)

```
<scene layout column center center>
  <icon hero />
  <text title />
  <text subtitle />
  <card row gap>
    <icon /> <text />
  </card>
</scene>
```

```json
{
  "layout": { "direction": "column", "align_items": "center", "justify_content": "center", "gap": 32 },
  "children": [
    { "type": "icon", "icon": "lucide:star", "style": { "width": 160, "height": 160, "color": "#6366F1" } },
    { "type": "text", "content": "Titre", "style": { "font-size": 96, "font-weight": "bold", "color": "#FFFFFF", "text-align": "center" } },
    { "type": "text", "content": "Description", "style": { "font-size": 48, "color": "#94A3B8", "text-align": "center" } }
  ]
}
```

### Grille de cards (2 colonnes)

```
<scene layout column center center>
  <text title />
  <card display:grid 2fr gap:24>
    <card /> <card />
    <card /> <card />
  </card>
</scene>
```

```json
{
  "layout": { "direction": "column", "align_items": "center", "justify_content": "center", "gap": 40 },
  "children": [
    { "type": "text", "content": "Features", "style": { "font-size": 80, "font-weight": "bold", "color": "#FFFFFF" } },
    {
      "type": "card",
      "style": {
        "width": 960, "height": 900,
        "display": "grid",
        "grid-template-columns": [{ "fr": 1 }, { "fr": 1 }],
        "grid-template-rows": [{ "fr": 1 }, { "fr": 1 }],
        "gap": 24, "padding": 0, "background": "#00000000"
      },
      "children": [
        { "type": "card", "style": { ... }, "children": [...] },
        { "type": "card", "style": { ... }, "children": [...] },
        { "type": "card", "style": { ... }, "children": [...] },
        { "type": "card", "style": { ... }, "children": [...] }
      ]
    }
  ]
}
```

### Contenu + fond décoratif

```
<scene layout column center center>
  <!-- fond : shapes absolus, ne participent PAS au flux -->
  <shape circle absolute z-index=0 />
  <shape circle absolute z-index=0 />

  <!-- contenu : flux normal, centré automatiquement -->
  <card z-index=1>...</card>
</scene>
```

```json
{
  "layout": { "direction": "column", "align_items": "center", "justify_content": "center" },
  "children": [
    { "type": "shape", "shape": "circle", "position": "absolute", "x": 100, "y": 400,
      "fill": { "type": "radial", "colors": ["#6366F190", "#6366F100"] },
      "style": { "z-index": 0, "width": 800, "height": 800 } },
    { "type": "card", "style": { "z-index": 1, "width": 900, ... }, "children": [...] }
  ]
}
```

---

## Erreurs communes

### Tout en absolu (anti-pattern canvas)

```json
// ❌ — pense en éditeur graphique, pas en HTML
{ "type": "text", "content": "Titre", "position": "absolute", "x": 190, "y": 400 }
{ "type": "text", "content": "Sous-titre", "position": "absolute", "x": 190, "y": 560 }
{ "type": "card", "position": "absolute", "x": 90, "y": 700, "style": { "width": 900 } }
```

```json
// ✅ — pense en HTML/CSS, flux naturel
{ "layout": { "direction": "column", "align_items": "center", "justify_content": "center", "gap": 32 },
  "children": [
    { "type": "text", "content": "Titre", "style": { "font-size": 96, "text-align": "center" } },
    { "type": "text", "content": "Sous-titre", "style": { "font-size": 48, "text-align": "center" } },
    { "type": "card", "style": { "width": 900 }, "children": [...] }
  ]
}
```

### Shape décoratif sans absolute (push le contenu)

```json
// ❌ — le circle consomme de l'espace dans le flex column
// et pousse la card vers le bas de l'écran
{ "type": "shape", "shape": "circle", "fill": {...}, "style": { "width": 800, "height": 800 } }
{ "type": "card", "style": { "width": 900, "height": 560 }, "children": [...] }
```

```json
// ✅ — retiré du flux, inoffensif
{ "type": "shape", "shape": "circle", "position": "absolute", "x": 100, "y": 400, "fill": {...},
  "style": { "z-index": 0, "width": 800, "height": 800 } }
{ "type": "card", "style": { "z-index": 1, "width": 900, "height": 560 }, "children": [...] }
```
