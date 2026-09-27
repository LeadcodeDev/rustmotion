# Profondeur de champ : `camera.focus` et `camera.aperture`

`style.depth` sert déjà à la parallaxe : un élément à `depth: 3` se déplace trois
fois plus que le décor quand la caméra bouge. Les deux mêmes chiffres pilotent
maintenant la netteté.

| Champ | Défaut | Rôle |
|---|---|---|
| `camera.focus` | `1.0` | la profondeur qui est nette, sur l'échelle de `style.depth` |
| `camera.aperture` | `0.0` | pixels de flou par unité d'écart, donc `0` = tout net |

```
sigma = aperture × |depth − focus|
```

C'est linéaire et symétrique : un plan deux unités devant est aussi flou qu'un
plan deux unités derrière.

```json
"camera": {
  "aperture": 7.0,
  "focus": 1.0,
  "keyframes": [{ "property": "focus", "values": [
    { "time": 0.0, "value": 1.0 },
    { "time": 2.0, "value": 3.0 }
  ]}]
}
```

`focus` et `aperture` s'animent par `keyframes` comme `zoom` et `rotation`.
Animer `focus` donne un **rack focus** — la mise au point glisse d'un plan à
l'autre. Animer `aperture` ouvre et ferme l'effet sans déplacer le plan net.

## Ordres de grandeur

`aperture: 4` sépare visiblement sans gêner la lecture. Au-delà de `20`, un plan
hors focus devient un aplat de couleur — utile pour un fond, pas pour du texte
qu'on doit encore reconnaître.

## Trois choses à savoir

**Rien ne bouge sans profondeurs distinctes.** La caméra par plans n'est résolue
que si au moins un enfant déclare `style.depth` ; et comme `depth` et `focus`
valent tous deux `1.0` par défaut, un scénario qui ne mentionne ni l'un ni l'autre
est net partout. `aperture: 0` rend **exactement** les mêmes octets qu'avant que
la fonctionnalité existe.

**La profondeur se lit au premier niveau.** Comme la parallaxe, le flou s'applique
aux enfants directs de la scène : un plan est une couche, pas un nœud isolé au
fond d'un sous-arbre. Mettre `depth` sur un enfant profond ne le défocalise pas
tout seul.

**Le flou déborde de la boîte.** La couche de peinture est élargie de `3 × sigma`
pour que le dégradé ne soit pas coupé net au bord. Un `overflow: hidden` sur le
parent, lui, le coupera — c'est la sémantique CSS attendue, pas un bug.
