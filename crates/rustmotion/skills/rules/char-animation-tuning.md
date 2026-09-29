# Rule: Tuning a per-character or per-word animation

The seven `char_*` presets (`char_scale_in`, `char_fade_in`, `char_wave`, `char_bounce`, `char_rotate_in`, `char_slide_up`, `char_blur_in`) share one config. Six fields tune it, all optional, **all defaulting to the historical behaviour**: an existing scenario doesn't move.

```json
{
  "type": "text",
  "content": "CASCADE",
  "style": {
    "font-size": 92,
    "animation": [{
      "name": "char_slide_up",
      "direction": "down",
      "distance": 1.6,
      "scale_from": 0.9,
      "duration": 0.5,
      "stagger": 0.035
    }]
  }
}
```

| Field | Role | Default |
|---|---|---|
| `direction` | `up` / `down` / `left` / `right` — where the unit arrives from | `up` |
| `distance` | Displacement multiplier (0.5 tight, 1.85 pronounced) | `1.0` |
| `scale_from` | Starting scale of each unit (0.82 = "pop", 0.92 = barely) | absent |
| `jitter` + `seed` | Deterministic irregularity of the `stagger` | `0` |
| `ink_from` | Starting colour, converges to `style.color` | absent |
| `blur` | Starting sigma (`char_blur_in` only) | `14` |

## What each preset actually reads

`direction` and `distance` only make sense for presets whose motion **is** a translation: `char_slide_up` and `char_blur_in`. The others (`scale_in`, `bounce`, `rotate_in`, `fade_in`, `wave`) have no displacement axis to redirect, and ignore them.

`scale_from` **composes** with the preset instead of replacing it — except on `char_scale_in` and `char_bounce`, which already own their own scale curve and ignore it (two stacked scale curves fight each other instead of composing).

## The name `char_slide_up` doesn't constrain the direction

`char_slide_up` with `"direction": "down"` makes the letters fall from above. The name is historical: it's the "translation" preset, and `up` is its default. There is no `char_slide_down`.

## `granularity` decides what a unit is

`"granularity": "word"` animates words, `"char"` (default) animates characters. A 40-character title animated at `char` granularity with `stagger: 0.05` takes 2s to settle before even hitting its own `duration` — count the number of units before picking a `stagger`, or switch to `word`.

## A note on `char_blur_in`

It goes through the same resolution path as its six siblings: it **inherits** `stagger` from a parent container and works inside a `timeline` step. (That wasn't always the case.)

## `rotate_from`, les deux jitters, et `reflow`

| Champ | Rôle | Défaut |
|---|---|---|
| `rotate_from` (+ `rotate_origin`) | Inclinaison de départ en degrés, redressée à 0 à la fin de l'animation de l'unité. `rotate_origin: "center"` (défaut) ou `"edge"` — charnière sur le bord d'attaque, c'est ce qui donne l'escalier. Inerte sur `char_rotate_in`, qui possède déjà cet axe. | absent |
| `scale_jitter` / `baseline_jitter` (+ `seed`) | Bruit déterministe par unité sur la taille et la ligne de base, qui retombe à 0 à la fin. Indépendant de `jitter`, qui ne décale que le timing — les trois tirent du même hash sous des sels différents, donc ils ne bougent pas en bloc pour une même graine. | absent |
| `reflow` (+ `reflow_easing`) | Une unité pas encore démarrée ne contribue aucune largeur à sa ligne, et la contribution d'une unité qui démarre entre via `reflow_easing`. Une ligne centrée reste donc centrée pendant qu'elle s'écrit, au lieu de réserver sa largeur finale dès la première frame. | `false` |

`reflow` modifie **ce qu'une unité occupe**, pas ce qu'elle peint : les glyphes se dessinent à pleine taille, seule leur position dans la ligne s'anime. Un glyphe peut donc chevaucher brièvement son voisin pendant la transition — c'est le prix à payer pour que chaque preset garde son propre visuel intact.

> `reflow` **n'a aucun effet sur `rich_text`**, dont le rendu d'animation par caractère calcule ses positions de tokens et ses fonds de pastille en une passe, avant l'animation. Le validateur le signale plutôt que de laisser le champ paraître actif. Pour une ligne centrée qui doit rester centrée pendant qu'elle s'écrit, c'est `text` ou `gradient_text`.
