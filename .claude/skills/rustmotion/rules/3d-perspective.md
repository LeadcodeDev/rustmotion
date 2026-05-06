# Rule: 3D Perspective Transforms

## Résumé

Le moteur utilise un pipeline M44 (Skia 4×4) pour le rendu 3D natif. Toute combinaison de `rotate_x`, `rotate_y`, `perspective` (via presets ou keyframes) active automatiquement ce pipeline — la 2D reste sur le chemin rapide.

---

## Deux façons d'activer la 3D

### 1. Presets (recommandé)

Les presets gèrent `rotate_x`, `rotate_y`, `perspective` et `opacity` en une seule ligne :

| Preset | Effet | Usage typique |
|---|---|---|
| `tilt_in` | Entrée depuis 15°/−15° avec scale 0.9→1 et fade | Hero card, carte principale |
| `float_3d` | Oscillation continue ±5° X / ±8° Y + léger translate_y | Loop après entrée |
| `flip_in_x` | Rotation X depuis 90° avec fade | Révélation dramatique |
| `flip_in_y` | Rotation Y depuis 90° avec fade | Révélation côté |
| `flip_out_x` | Rotation X vers −90° avec fade | Exit de scène |
| `flip_out_y` | Rotation Y vers −90° avec fade | Exit de scène |

**Pattern d'entrée + flottement continu :**
```json
{
  "style": {
    "animation": [
      { "name": "tilt_in", "duration": 1.0 },
      { "name": "float_3d", "loop": true }
    ]
  }
}
```

`float_3d` avec `"loop": true` commence à t=0 et boucle indéfiniment sur la durée de la scène. Il se compose naturellement avec `tilt_in` car les propriétés sont additives.

### 2. Keyframes (contrôle total)

Utilise la propriété `"name": "keyframes"` avec les propriétés `rotate_x`, `rotate_y`, `perspective` :

```json
{
  "style": {
    "animation": [
      {
        "name": "keyframes",
        "keyframes": [
          {
            "property": "rotate_x",
            "keyframes": [{ "time": 0, "value": 20 }, { "time": 2, "value": 0 }],
            "easing": "ease_out"
          },
          {
            "property": "rotate_y",
            "keyframes": [{ "time": 0, "value": -12 }, { "time": 2, "value": 0 }],
            "easing": "ease_out"
          },
          {
            "property": "perspective",
            "keyframes": [{ "time": 0, "value": 800 }, { "time": 2, "value": 800 }],
            "easing": "linear"
          }
        ]
      }
    ]
  }
}
```

**Toujours déclarer `perspective` dans les keyframes** — sinon la rotation n'a pas de profondeur.

---

## Propriétés 3D

| Propriété | Description | Plage recommandée |
|---|---|---|
| `rotate_x` | Inclinaison avant/arrière (axe X) | −30 à 30 degrés |
| `rotate_y` | Inclinaison gauche/droite (axe Y) | −30 à 30 degrés |
| `perspective` | Distance de la caméra (plus bas = plus dramatique) | 400–1200 px |

Les rotations >30° déforment le contenu et rendent le texte illisible. Rester dans −30..30.

---

## Shadow 3D adaptatif (automatique)

Quand un composant a un `box-shadow` **et** une rotation 3D active, le shadow se décale et se scale automatiquement en fonction des angles de tilt. Aucune config supplémentaire.

```json
{
  "style": {
    "box-shadow": [
      { "color": "#00000070", "offset-y": 40, "blur": 80 },
      { "color": "#6366F130", "offset-y": 20, "blur": 60 }
    ],
    "animation": [
      { "name": "tilt_in", "duration": 1.0 },
      { "name": "float_3d", "loop": true }
    ]
  }
}
```

---

## Exemple complet validé

```json
{
  "version": "1.0",
  "video": { "width": 1080, "height": 1920, "fps": 30, "background": "#0a0e1a" },
  "scenes": [
    {
      "duration": 6.0,
      "layout": { "direction": "column", "align_items": "center", "justify_content": "center" },
      "children": [
        {
          "type": "shape", "shape": "circle",
          "position": "absolute", "x": 60, "y": 420,
          "fill": { "type": "radial", "colors": ["#6366F190", "#6366F100"] },
          "style": {
            "z-index": 0, "width": 900, "height": 900,
            "animation": [
              { "name": "fade_in", "duration": 1.5 },
              { "name": "wiggle", "property": "scale", "amplitude": 0.06, "frequency": 0.18, "seed": 7 }
            ]
          }
        },
        {
          "type": "card",
          "style": {
            "width": 920, "height": 580,
            "background": "#FFFFFF14",
            "backdrop-filter": [{ "fn": "blur", "radius": 28 }],
            "border-radius": 44,
            "border": { "width": 1, "style": "solid", "color": "#FFFFFF28" },
            "box-shadow": [
              { "color": "#00000070", "offset-y": 40, "blur": 80 },
              { "color": "#6366F130", "offset-y": 20, "blur": 60 }
            ],
            "flex-direction": "column", "align-items": "center",
            "justify-content": "center", "gap": 32, "padding": 64,
            "z-index": 1,
            "animation": [
              { "name": "tilt_in", "duration": 1.0 },
              { "name": "float_3d", "loop": true }
            ]
          },
          "children": [
            {
              "type": "icon", "icon": "lucide:layers-3",
              "style": {
                "color": "#A5B4FC", "width": 96, "height": 96,
                "animation": [{ "name": "scale_in", "delay": 0.5, "duration": 0.5 }]
              }
            },
            {
              "type": "text", "content": "3D Perspective",
              "style": {
                "font-size": 80, "font-weight": "bold",
                "color": "#FFFFFF", "text-align": "center",
                "animation": [{ "name": "fade_in_up", "delay": 0.6, "duration": 0.6 }]
              }
            },
            {
              "type": "text", "content": "Rendu M44 natif Skia",
              "style": {
                "font-size": 40, "color": "#94A3B8", "text-align": "center",
                "animation": [{ "name": "fade_in_up", "delay": 0.8, "duration": 0.5 }]
              }
            }
          ]
        }
      ]
    }
  ]
}
```

---

## Règles de composition

**Les éléments décoratifs de fond doivent toujours être absolus.** Un shape ou particle sans `position: "absolute"` participe au flex flow de la scène et décale le contenu centré.

```json
// BAD — pousse la card vers le bas
{ "type": "shape", "shape": "circle", "fill": { ... }, "style": { "width": 800, "height": 800 } }

// GOOD — reste en fond, ne perturbe pas le flux
{ "type": "shape", "shape": "circle", "position": "absolute", "x": 60, "y": 420, "fill": { ... }, "style": { "width": 800, "height": 800 } }
```

**`perspective` dans `style` vs dans les keyframes :** Le champ `css.perspective` (réglé automatiquement par les presets et le keyframe `"property": "perspective"`) active le pipeline M44 même sans rotation — il n'est jamais nécessaire de le déclarer manuellement dans `style`.

---

## Tips

- `perspective: 800` → dramatique. `perspective: 1000–1200` → subtil. Ne pas dépasser 1200.
- Combiner `tilt_in` (0.8–1.2s) + `float_3d loop` = pattern hero card standard.
- Ajouter un `box-shadow` avec `color` semi-transparent pour le shadow adaptatif gratuit.
- `flip_in_x` / `flip_in_y` : utiliser avec `duration: 0.6–0.8` pour un effet snappy.
- Pour une grille de cards 3D : appliquer `tilt_in` avec `stagger: 0.15` à chaque card.
