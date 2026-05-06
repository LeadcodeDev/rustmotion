# Rule: HTML/CSS Mental Model

## Principe

L'API JSON de Rustmotion est un superset de HTML/CSS. Chaque scène est un flex container, chaque `card` est une `<div>`, les propriétés CSS (`gap`, `padding`, `flex-direction`, `align-items`, etc.) ont exactement la même sémantique.

**Avant d'écrire du JSON, se poser cette question : "Comment j'écrirais ça en HTML/CSS ?"**

---

## Correspondances directes

### Conteneurs layout

| HTML/CSS | Rustmotion JSON | Quand l'utiliser |
|---|---|---|
| `<div>` neutre | `{"type": "div"}` | Layout pur, sans décoration visuelle |
| `<div class="card">` avec fond, border-radius | `{"type": "card"}` | Container avec styling visuel |
| `<div style="display:flex; flex-direction:row; gap:24px">` | `{"type": "div", "style": {"flex-direction": "row", "gap": 24}}` | Ligne horizontale |
| `<div style="display:grid; grid-template-columns:1fr 1fr">` | `{"type": "div", "style": {"display": "grid", "grid-template-columns": ["1fr","1fr"]}}` | Grille |

**Règle de choix** :
- `div` → grouper des enfants sans fond, border-radius, ou ombre. Flex par défaut.
- `card` → conteneur avec background, border-radius, shadow. Flex par défaut.
- Les deux acceptent les mêmes propriétés CSS (`gap`, `padding`, `flex-direction`, `align-items`, `justify-content`, `grid-template-columns`, etc.)

### Autres correspondances

| HTML/CSS | Rustmotion JSON |
|---|---|
| `<body>` avec `display:flex; flex-direction:column; align-items:center; justify-content:center` | `"layout": {"direction": "column", "align_items": "center", "justify_content": "center"}` |
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

## Positionnement relatif : les outils

Tout ce qui est **espace, alignement, distribution** se règle via les propriétés du parent — jamais via `x`/`y` sur l'enfant.

### Référence rapide

| Besoin | Propriété | Sur quel élément | Exemple |
|---|---|---|---|
| Espace entre enfants frères | `gap` | Parent (flex ou grid) | `"gap": 24` |
| Espace entre contenu et bordure du container | `padding` | Le container lui-même | `"padding": 40` ou `"padding": [32, 48]` |
| Décaler UN seul enfant par rapport aux autres | `margin` | L'enfant en question | `"margin-top": 16` |
| Centrer horizontalement (axe principal = column) | `align-items: "center"` | Parent flex | `"align-items": "center"` |
| Centrer verticalement (axe principal = column) | `justify-content: "center"` | Parent flex | `"justify-content": "center"` |
| Pousser un enfant à droite | `margin-left: "auto"` | Cet enfant | `"margin-left": "auto"` |
| Élément prend tout l'espace restant | `flex-grow: 1` | L'enfant | `"flex-grow": 1` |
| Alignement différent pour un seul enfant | `align-self` | L'enfant | `"align-self": "flex-end"` |
| 2 colonnes égales | `grid-template-columns` | Parent grid | `["1fr","1fr"]` |
| 3 colonnes proportionnelles | `grid-template-columns` | Parent grid | `["2fr","1fr","1fr"]` |
| Colonne de taille fixe + reste | `grid-template-columns` | Parent grid | `[240, "1fr"]` |

### Règle de décision

```
Besoin d'espace entre deux éléments ?
  → gap sur le parent  (jamais margin-bottom sur le premier)

Besoin d'espace à l'intérieur d'un container ?
  → padding sur le container  (jamais positionner les enfants manuellement)

Besoin de centrer ?
  → align-items + justify-content sur le parent  (jamais x/y calculés à la main)

Besoin qu'un élément prenne tout l'espace disponible ?
  → flex-grow: 1 sur cet élément  (jamais width calculée manuellement)

Besoin d'une exception pour UN seul enfant ?
  → align-self / margin sur cet enfant  (pas un container supplémentaire)
```

### Exemples concrets

**Espace entre un titre et une card :**

```json
// ❌ — deux éléments absolus, espace calculé à la main
{ "type": "text", "content": "Titre", "position": "absolute", "x": 560, "y": 200 }
{ "type": "card", "position": "absolute", "x": 90, "y": 360, "style": { "width": 900 } }

// ✅ — gap dans le parent, rien à calculer
{
  "layout": { "direction": "column", "align_items": "center", "justify_content": "center", "gap": 40 },
  "children": [
    { "type": "text", "content": "Titre", "style": { "font-size": 80, "text-align": "center" } },
    { "type": "card", "style": { "width": 900, "padding": 40 }, "children": [...] }
  ]
}
```

**Icône + texte alignés horizontalement :**

```json
// ❌ — positions absolues calculées
{ "type": "icon", "position": "absolute", "x": 40, "y": 50 }
{ "type": "text", "content": "Feature", "position": "absolute", "x": 108, "y": 58 }

// ✅ — flex row, gap, align-items
{
  "type": "card",
  "style": { "flex-direction": "row", "align-items": "center", "gap": 16, "padding": 24 },
  "children": [
    { "type": "icon", "icon": "lucide:check", "style": { "width": 48, "height": 48 } },
    { "type": "text", "content": "Feature", "style": { "font-size": 36 } }
  ]
}
```

**Badge poussé à droite dans une card :**

```json
// ❌ — badge en absolute, coordonnées fragiles
{ "type": "badge", "content": "NEW", "position": "absolute", "x": 780, "y": 20 }

// ✅ — div row, le badge utilise margin-left: auto
{
  "type": "div",
  "style": { "flex-direction": "row", "align-items": "center", "width": 900 },
  "children": [
    { "type": "text", "content": "Titre", "style": { "font-size": 48, "flex-grow": 1 } },
    { "type": "badge", "content": "NEW", "style": { "background": "#6366F1" } }
  ]
}
```

**Section avec padding asymétrique :**

```json
// ❌ — enfants positionnés pour simuler du padding
{ "type": "text", "position": "absolute", "x": 60, "y": 40 }

// ✅ — padding sur le container, les enfants sont en flow
{ "type": "card", "style": { "padding": [40, 60], "gap": 24, "width": 900 }, "children": [...] }
//                                           ↑top/bottom  ↑left/right
```

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
        "grid-template-columns": ["1fr", "1fr"],
        "grid-template-rows": ["1fr", "1fr"],
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
