# Rule: `motion_blur` et `trail` — les fantômes ne sont plus des enfants du flex

`motion_blur` et `trail` (`style.animation`) peignent des copies fantômes du composant à des instants antérieurs (`BoxKind::Ghost` dans `box_builder.rs`), avec une opacité décroissante. Jusqu'à la régression de l'issue #359, ces fantômes se comportaient mal sur trois points distincts. Les trois sont corrigés ; ce fichier documente le comportement actuel et ce qui reste volontairement hors scope.

```json
{
  "type": "div",
  "style": {
    "width": 200, "height": 60, "background": "#FF4FB0",
    "animation": [{ "name": "motion_blur", "samples": 8, "shutter": 1.0 }]
  }
}
```

## 1. Un fantôme ne prend jamais de place dans le flex

Avant la correction, un fantôme d'un enfant **en flux** (sans `position: absolute`) devenait lui-même un item flex à part entière — `samples` copies pleine taille en plus de l'élément réel, qui poussaient les frères suivants hors cadre. Un fantôme est maintenant systématiquement `position: absolute`, que le nœud qu'il duplique soit lui-même en flux ou déjà positionné :

- Nœud déjà `position: absolute` → le fantôme reprend exactement son `left`/`top` (comportement inchangé).
- Nœud en flux → le fantôme n'a pas d'inset explicite ; Taffy le positionne alors selon `justify-content`/`align-items` du conteneur, comme n'importe quel enfant absolu sans `top`/`left` — il ne consomme aucun slot et ne déplace aucun frère, même si la position exacte du fantôme peut légèrement différer de celle du principal dans une mise en page asymétrique (`space-between`, plusieurs frères de tailles différentes). Le principal, lui, reste résolu par le flex normalement.

## 2. Le fantôme d'un conteneur porte son propre sous-arbre

Un fantôme n'est plus construit avec `children: Vec::new()`. Un `div` avec un fond et un enfant `text` voit maintenant les deux dupliqués — le sous-arbre est reconstruit à l'instant propre du fantôme (via `container_children`), pas simplement recopié depuis le principal : un enfant qui a sa propre animation (délai, keyframes) est donc rejoué à l'instant du fantôme, pas à l'instant courant de la scène. C'est la brique qui permet à une carte ou un mockup entier de traîner comme une unité.

Un fantôme d'un composant **mesuré** (`text`, `counter`, `badge`, `table`, `rich_text`, `kbd`, `caption`, `number_wheel`) porte aussi son propre `intrinsic` — sans quoi la boîte se mesurait à zéro et rien ne se peignait, exactement le symptôme "le texte n'a aucune traînée, la forme d'à côté oui" de l'issue.

> Pas de champ `scope: "self" | "subtree"` pour choisir de ne fantômer que la boîte du conteneur sans ses enfants — chaque fantôme d'un conteneur embarque systématiquement tout son sous-arbre. Aucun cas d'usage vérifié n'en a besoin ; à ajouter si un scénario réel le demande.

## 3. `pointer.path` est échantillonné par fantôme

Un `pointer` dont le déplacement vient de `path` (pas de `translate_x`/`translate_y` en keyframes) calcule sa position dans son propre `paint_content`, à partir de `ctx.time` — pas via `style.transform`. Chaque fantôme reçoit maintenant sa propre horloge locale (`time_params`, la même table que celle qui pilote `stagger_offset`), décalée exactement de l'écart temporel de cet échantillon. Un pointeur dont le trajet est piloté par `path` laisse donc une traînée le long de sa trajectoire, comme un pointeur piloté par keyframes.

## `mode: "smear"` — pas de fantômes du tout

```json
{ "name": "motion_blur", "mode": "smear", "shutter": 1.0 }
```

Au lieu d'empiler des copies (`mode: "stack"`, le défaut), `smear` mesure le déplacement du composant sur la fenêtre `shutter / fps` qui précède l'instant courant, et pose directement un filtre `{ "fn": "blur", "radius-x": …, "radius-y": … }` sur le principal — voir [rules/directional-blur.md](directional-blur.md). Zéro nœud fantôme créé : `samples` est ignoré en mode `smear`. C'est la solution recommandée pour un déplacement rapide et rectiligne (un mot qui traverse le cadre) — un vrai flou directionnel au lieu d'un escalier de copies visibles à haute vitesse.

`samples: "auto"` (densifier les copies de `mode: "stack"` jusqu'à moins de 2px d'écart entre elles, mentionné dans l'issue #360 comme filet de sécurité si un vrai noyau de flou n'était pas atteignable) n'est pas implémenté — `mode: "smear"` couvre ce besoin directement, `samples` reste un entier `1..=16`.
