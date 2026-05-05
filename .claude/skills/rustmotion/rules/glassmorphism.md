# Rule: Glassmorphism — Glass Cards et Surfaces Translucides

Le glassmorphisme crée une illusion de verre dépoli : le fond est visible mais flou à travers la surface. L'effet repose sur **4 ingrédients combinés** — manquer l'un d'eux casse l'illusion.

---

## Les 4 ingrédients obligatoires

| Ingrédient | Propriété | Valeur recommandée |
|---|---|---|
| **Flou de fond** | `backdrop-filter` | `[{ "fn": "blur", "radius": 24 }]` |
| **Fond semi-transparent** | `background` (hex 8 chiffres) | `#FFFFFF14` à `#FFFFFF28` sur fond sombre |
| **Bordure translucide** | `border` | `{ "width": 1, "style": "solid", "color": "#FFFFFF30" }` |
| **Ombre diffuse** | `box-shadow` | `[{ "color": "#00000040", "offset-y": 24, "blur": 48 }]` |

Sans `backdrop-filter`, la carte est juste translucide, pas en verre.
Sans fond semi-transparent, l'effet est opaque.
Sans bordure, la carte flotte sans contour visible.
Sans ombre, elle ne se détache pas du fond.

**Propriété correcte : `backdrop-filter`, pas `backdrop-blur`.**
`backdrop-blur` n'existe pas dans CssStyle — utiliser `backdrop-filter` avec `{ "fn": "blur", "radius": N }`.

---

## Opacité du fond selon la couleur de fond de scène

| Fond de scène | Background card recommandé | Résultat |
|---|---|---|
| Sombre (`#0f172a`, `#0a0a1a`) | `#FFFFFF12` – `#FFFFFF20` | Verre subtil (dark UI) |
| Moyen (`#1e293b`, `#1a1a2e`) | `#FFFFFF20` – `#FFFFFF30` | Verre standard |
| Clair (`#f0f4f8`, `#ffffff`) | `#FFFFFF60` – `#FFFFFF90` | Frosted glass (light UI) |
| Coloré (dégradé vif derrière) | `#FFFFFF18` – `#FFFFFF28` | Maximum de couleur visible |

Règle du pouce : **hex opacity 12–30 sur fond sombre, 60–90 sur fond clair.**

---

## Backdrop-filter blur : plages d'utilisation

| Intensité | `radius` | Utilisation |
|---|---|---|
| Subtil | 8–16 | Cartes d'interface légère |
| Standard | 20–32 | **Recommandé** — verre lisible sans excès |
| Fort | 40–60 | Effet dramatique, fond peu visible |
| Extrême | 80+ | Fond quasi-invisible — réserver aux overlays modaux |

---

## Template complet — glass card sur fond sombre

```json
{
  "type": "card",
  "size": { "width": 860, "height": 480 },
  "animation": [
    { "name": "fade_in_up", "duration": 0.6 },
    { "name": "float_3d", "loop": true }
  ],
  "style": {
    "background": "#FFFFFF18",
    "backdrop-filter": [{ "fn": "blur", "radius": 24 }],
    "border-radius": 32,
    "border": { "width": 1, "style": "solid", "color": "#FFFFFF30" },
    "box-shadow": [
      { "color": "#00000050", "offset-y": 32, "blur": 64 },
      { "color": "#FFFFFF08", "offset-y": -1, "blur": 0, "inset": true }
    ],
    "flex-direction": "column",
    "align-items": "center",
    "justify-content": "center",
    "gap": 24,
    "padding": 48,
    "z-index": 1
  },
  "children": [...]
}
```

Le `box-shadow` inset blanc (`offset-y: -1`) simule un reflet de lumière en haut de la carte — touche finale qui renforce l'effet verre.

---

## Fond : les éléments qui brillent à travers le verre

Le glassmorphisme n'a d'intérêt que s'il y a quelque chose à voir derrière. Placer des blobs colorés **derrière** la carte (`z-index: 0`) :

```json
{
  "type": "shape",
  "shape": "circle",
  "size": { "width": 700, "height": 700 },
  "position": "absolute",
  "x": 90,
  "y": 600,
  "fill": { "type": "radial", "colors": ["#6366F180", "#6366F100"] },
  "animation": [
    { "name": "fade_in", "duration": 1.0 },
    { "name": "wiggle", "property": "scale", "amplitude": 0.06, "frequency": 0.25, "seed": 7 },
    { "name": "wiggle", "property": "translate_y", "amplitude": 14, "frequency": 0.20, "seed": 91 }
  ],
  "style": { "z-index": 0 }
}
```

- Opacity blobs : **≥ 50% (`80` en hex)** sur fond sombre — sinon invisibles à travers le flou.
- Minimum 2 blobs de couleurs différentes, positions opposées.
- Amplitudes wiggle plus grandes que d'habitude (le flou masque les micro-mouvements).
- Positionner les blobs de sorte que `x >= 0` et `x + width <= viewport_width` — le validateur rejette tout débordement, même pour les décoratifs.

---

## Texte sur verre : règles de lisibilité

Le verre translucide réduit le contraste. Compenser :

| Élément | Recommandation |
|---|---|
| Titre | `color: "#FFFFFF"`, `font-weight: bold` |
| Sous-titre | `color: "#E2E8F0"` (pas pur blanc) |
| Corps | `color: "#CBD5E1"` — éviter `#94A3B8` (trop peu de contraste) |
| Icône | `color: "#FFFFFF"` ou teinte accent forte |

Ne pas mettre `opacity` sur le texte lui-même — ça composerait avec le fond semi-transparent et tuerait la lisibilité.

---

## Stacking glass : éviter l'empilement de flous

Deux cartes glass superposées multiplient les `backdrop-filter` — résultat : flou excessif, fond entièrement masqué, effet perdu.

**BAD — double verre empilé :**
```json
[
  { "type": "card", "style": { "backdrop-filter": [{ "fn": "blur", "radius": 24 }], "background": "#FFFFFF18" },
    "children": [
      { "type": "card", "style": { "backdrop-filter": [{ "fn": "blur", "radius": 20 }], "background": "#FFFFFF15" } }
    ]
  }
]
```

**GOOD — un seul verre, fond opaque pour les enfants :**
```json
[
  { "type": "card", "style": { "backdrop-filter": [{ "fn": "blur", "radius": 24 }], "background": "#FFFFFF18" },
    "children": [
      { "type": "card", "style": { "background": "#FFFFFF10", "border-radius": 16 } }
    ]
  }
]
```

---

## Variantes par palette

### Dark Tech (navy + indigo)
```json
{
  "background": "#FFFFFF15",
  "backdrop-filter": [{ "fn": "blur", "radius": 24 }],
  "border": { "width": 1, "style": "solid", "color": "#6366F130" },
  "box-shadow": [{ "color": "#6366F120", "offset-y": 0, "blur": 60 }]
}
```
La bordure et l'ombre reprennent la teinte accent — le verre a une couleur.

### Frosted White (fond clair)
```json
{
  "background": "#FFFFFF70",
  "backdrop-filter": [{ "fn": "blur", "radius": 20 }],
  "border": { "width": 1, "style": "solid", "color": "#FFFFFF90" },
  "box-shadow": [{ "color": "#00000018", "offset-y": 8, "blur": 24 }]
}
```

### Aurora (fond dégradé multicolore)
```json
{
  "background": "#FFFFFF12",
  "backdrop-filter": [{ "fn": "blur", "radius": 32 }],
  "border": { "width": 1, "style": "solid", "color": "#FFFFFF20" },
  "box-shadow": [
    { "color": "#00000060", "offset-y": 40, "blur": 80 },
    { "color": "#FFFFFF0A", "offset-y": -1, "blur": 0, "inset": true }
  ]
}
```

---

## BAD : verre sans fond coloré derrière

```json
{
  "video": { "background": "#0f172a" },
  "scenes": [{
    "children": [
      { "type": "card", "style": { "backdrop-filter": [{ "fn": "blur", "radius": 24 }], "background": "#FFFFFF18" } }
    ]
  }]
}
```

Fond uni `#0f172a` + verre → on ne voit rien à travers le flou, la carte est juste grise. **Toujours placer des blobs colorés z-index: 0 derrière.** ✗

## BAD : `backdrop-filter` sur un enfant de card

`backdrop-filter` floute ce qui est **derrière le composant dans le viewport**, pas ce qui est dans le parent. Si appliqué à un `text` ou `icon` enfant d'une carte opaque, il n'a aucun effet visible.

```json
{ "type": "text", "style": { "backdrop-filter": [{ "fn": "blur", "radius": 20 }], "color": "#fff" } }
```
Aucun effet — `backdrop-filter` est uniquement pertinent sur les éléments directement superposés à une texture/image/blob de fond. ✗
