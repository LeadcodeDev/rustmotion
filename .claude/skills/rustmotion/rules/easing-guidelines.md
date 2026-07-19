# Rule: Use Appropriate Easing for Motion Design

| Use case | Recommended easing |
|---|---|
| UI element entrances | `ease_out` or `ease_out_cubic` |
| Element exits | `ease_in` or `ease_in_cubic` |
| Continuous/looping | `linear` |
| Playful bouncy motion | `spring` with `{ "damping": 15, "stiffness": 100, "mass": 1 }` |
| Counter number animation | `ease_out` |
| Camera-like zoom | `ease_in_out` |
| Smooth subtle reveals | `spring` with `{ "damping": 200 }` |

Entrance presets already use appropriate easing internally.

## Available Easing Functions

`linear`, `ease_in`, `ease_out`, `ease_in_out`, `ease_in_quad`, `ease_out_quad`, `ease_in_cubic`, `ease_out_cubic`, `ease_in_expo`, `ease_out_expo`, `spring`

### Spring Physics

When easing is `spring`, configure with:
```json
{
  "easing": "spring",
  "spring": { "damping": 15, "stiffness": 100, "mass": 1 }
}
```

**Tout preset accepte `spring`.** Ajouter un objet `spring` à n'importe quel preset applique la physique de ressort à ses keyframes de mouvement (translate/scale/rotate) — l'opacity garde son ease (pas de flash d'alpha en overshoot) :

```json
{ "name": "fade_in_up", "duration": 0.8, "spring": { "damping": 8, "stiffness": 120 } }
```

- `bounce_in` / `elastic_in` : leurs springs intégrés sont les défauts ; un `spring` utilisateur les remplace.
- `scale_in` + spring : l'overshoot manuel est remplacé par celui du ressort.
- Oscillateurs continus (`pulse`, `shake`, `float`) : non affectés (leur forme est leur raison d'être).
- La `duration` reste la fenêtre de l'animation : le ressort est résolu en secondes réelles dans cette fenêtre et la valeur se cale sur la cible à la fin — choisir une duration suffisante (≥ 0.6s avec les défauts) pour laisser le ressort converger.
- Dialecte HTML : la DSL compacte accepte `spring:true` (défauts damping 15 / stiffness 100 / mass 1) ; la config fine passe par la forme JSON de `anim`.
