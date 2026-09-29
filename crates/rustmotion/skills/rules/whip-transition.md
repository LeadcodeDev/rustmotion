# Rule: `whip` — the directional motion-blurred pan

`whip` is a `transition` (like `slide`, `chromatic_wipe`, `zoom_blur`…): as with every transition in a `slide` view, it composites two **already rendered** frame buffers — no element survives the cut, only pixels are mixed. See the "Composition" section of `CLAUDE.md`.

The effect: a plain `slide` along an axis, whose displacement carries a motion streak peaking mid-transition — the outgoing scene stretches into a trail behind itself along `direction` as it fades, and the incoming scene arrives already streaked before settling, crisp, on arrival.

```json
{
  "transition": {
    "type": "whip",
    "direction": "left",
    "strength": 1.5,
    "duration": 0.35,
    "easing": "ease_in_out"
  }
}
```

The `transition` goes on the scene being **entered** — here, on the second scene, not the first. A transition on a view's first scene has nothing to come from, and the engine says so and ignores it:

```
Warning: the `transition` on view 0's first scene has no effect — a transition
belongs to the scene being entered, and the first scene has nothing to come from.
```

## Not to be confused with `slide`, `chromatic_wipe` or the `motion_blur` effect

- **`slide`** is the same displacement, dry, with no trail: `whip` with `strength: 0` is byte-identical to it at every instant of the transition.
- **`chromatic_wipe`** travels along the same axis (`direction` takes the same values), but its peak is a **chromatic** split (red/cyan) at the cut edge, not a spatial streak of the whole frame.
- **`motion_blur`** (an animation effect, `style.animation`) trails an **individual component**'s trajectory by accumulating samples of its own animation. It sees nothing inside a transition, which only has two already-painted RGBA buffers — exactly the gap `whip` fills on the transition side, as `zoom_blur` did for the radial zoom. See [rules/zoom-blur-transition.md](zoom-blur-transition.md).

## Fields

| Field | Role | Default |
|---|---|---|
| `direction` | Travel axis for both frames, same values as `slide`/`chromatic_wipe` (`left`/`right`/`up`/`down`). | `left` |
| `strength` | Reach of the trail. `0` removes the blur pass and leaves a dry `slide` — no trail at any instant, not even mid-transition. Larger values pull the copies further behind their current position. | `1.0` |
| `duration`, `easing` | Common to every transition. | `0.5`, `ease_in_out` |

## Zero at both ends, by construction

Like `zoom_blur` and `chromatic_wipe`, the intensity follows `peak = 1 - |2p - 1|`: zero at `progress = 0` and at `progress = 1`, whatever `strength` is. The engine does not let that curve merely tend to zero: at `reach <= 0.0` (so `peak == 0`, at both bounds) it returns the crisp slide directly, never building the trail pass at all — a short circuit, not a floating-point fade that could leave a rounding residue. `progress = 0` renders exactly the source frame, `progress = 1` exactly the destination frame: nothing bleeds into the next scene.

`strength: 0` takes the same short circuit at **every** instant of the transition, not only at the edges: the transition then degenerates into a dry `slide`, never laying down the trail pass.

## How it is built

The crisp base (`sharp`) is the same computation as `chromatic_wipe`'s internal slide — factored into `directional_slide` and shared by both transitions: two fully opaque frames, tiled side by side along `direction`, with no alpha mixing at all. When `strength` and the position in the transition call for it, a fine series of translated copies of **each** frame (120 of them, close enough together that no visible band survives) — the outgoing one AND the incoming one, unlike `zoom_blur`, which only trails the outgoing frame — are redrawn further and further behind their current position, at an opacity decreasing with distance. It is the same weighted sum of copies as `zoom_blur`, translated along an axis instead of scaled radially around an `origin`.

The step count is fixed rather than scaled to `reach`, and it costs: 120 steps means 240 `draw_image` calls per transition frame, measured at about 10 ms per frame at 1920×1080. Only transition frames pay it, so a 0.6 s whip adds roughly 0.2 s to a render.

> A truly reach-invariant result would need the trail computed as a real weight *integral* (additive accumulation normalised by sample density) rather than N sequential "over" composites, which saturate non-linearly. The fixed step count is what keeps two different `strength` values comparable: making the density follow `reach` gives them different sampling and breaks the "a bigger strength streaks further" property.

## Trap: a high `strength` bleeds the ghost onto the other scene

The translated copies are drawn over the whole frame, not only in the territory still owned by their own frame. A very high `strength` therefore bleeds a semi-transparent ghost of the outgoing scene into the area the incoming one already occupies, and vice versa — that is intended, and is precisely what reads as a motion streak crossing the cut rather than two images sliding over each other. If the effect looks too spread out, lower `strength` rather than `duration`: shortening the duration does not change `peak`'s maximum, only how fast the transition travels through it.
