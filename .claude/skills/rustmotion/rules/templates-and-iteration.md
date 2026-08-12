# Rule: Templates & Iteration

Ne duplique pas un sous-arbre. Si dix cartes ne diffèrent que par leurs données, écris-en une et itère.

C'est le mode d'échec le plus fréquent de la génération : dix copies écrites à la main, dont l'une finit par diverger sur une couleur, une `font-size` ou un `position` oublié.

## `for-each` — répéter un sous-arbre

Utilisable dans n'importe quel tableau `children`.

```json
{
  "for-each": [
    { "label": "Revenue", "value": 1250, "accent": "#22C55E" },
    { "label": "Users",   "value": 340,  "accent": "#3B82F6" }
  ],
  "template": {
    "type": "card",
    "style": { "width": "300px", "height": "160px", "background": "#111827" },
    "children": [
      { "type": "text", "content": "$label", "style": { "color": "$accent" } }
    ]
  }
}
```

Chaque élément du tableau **lie ses propres champs directement** : `$label`, `$value`, `$accent`. Il n'y a pas d'accès par chemin pointé — écrire `$item.label` ne fonctionne pas.

Deux liaisons sont fournies en plus :

| Liaison | Contenu |
|---|---|
| `$index` | La position dans le tableau, à partir de 0 |
| `$item` | L'élément entier, pour le transmettre tel quel |

Une donnée portant explicitement le nom `index` ou `item` gagne toujours sur la liaison intégrée.

Le `template` peut être un objet unique **ou un tableau** — dans ce cas ses éléments sont insérés comme des frères, pas imbriqués.

## `components` + `use` — définir une fois, instancier partout

Bloc racine, à côté de `scenes` / `composition` :

```json
{
  "components": {
    "stat_card": {
      "params": {
        "label":  { "type": "string" },
        "value":  { "type": "number", "default": 0 },
        "accent": { "type": "string", "default": "#6366F1" }
      },
      "template": {
        "type": "card",
        "children": [
          { "type": "text", "content": "$label", "style": { "color": "$accent" } }
        ]
      }
    }
  }
}
```

`params` a exactement la forme de `config` : `type`, `default`, `description`. **Omettre `default` rend le paramètre requis** — l'instancier sans le fournir est une erreur nommée, pas un défaut silencieux.

Instanciation :

```json
{ "use": "stat_card", "props": { "label": "Revenue", "value": 1250 } }
```

La clé s'appelle **`props`**, pas `config`. Ce n'est pas une inconsistance : la substitution de variables saute délibérément tout objet portant la clé `config`, pour protéger le bloc de déclarations racine. Utiliser ce nom ici laisserait tout `for-each` imbriqué dans un `use` silencieusement non substitué.

## Les deux se composent

C'est la forme la plus utile — une définition, une liste de données :

```json
{
  "for-each": [
    { "label": "Revenue", "value": 1250, "accent": "#22C55E" },
    { "label": "Users",   "value": 340,  "accent": "#3B82F6" },
    { "label": "Growth",  "value": 8,    "accent": "#F59E0B" }
  ],
  "template": {
    "use": "stat_card",
    "props": { "label": "$label", "value": "$value", "accent": "$accent" }
  }
}
```

## Le piège : un champ omis dans un élément

**Chaque élément d'un `for-each` doit fournir tous les champs que le template référence.**

Ceci ne fonctionne pas :

```json
"for-each": [
  { "label": "Revenue", "accent": "#22C55E" },
  { "label": "Users" }
],
"template": { "use": "stat_card", "props": { "label": "$label", "accent": "$accent" } }
```

Le second élément n'a pas d'`accent`, donc `$accent` reste littéral et le composant reçoit la chaîne `"$accent"` — pas son `default`. Un `default` de `params` s'applique quand `props` **omet la clé**, pas quand `props` transmet une liaison non résolue.

Ce n'est pas silencieux : la validation émet un avertissement de variable non résolue, et le composant qui consomme la valeur échoue à son tour (ici, `color '$accent' is not a recognized CSS color`). Mais corrige la donnée plutôt que le symptôme — remplis le champ dans chaque élément :

```json
"for-each": [
  { "label": "Revenue", "accent": "#22C55E" },
  { "label": "Users",   "accent": "#6366F1" }
]
```

## Ordre des passes, et ce qu'il autorise

Substitution des variables → expansion des directives → `include`, appliqué par document.

- **Tu peux** itérer sur un tableau venu d'une variable `config` ou de `--var` : la substitution tourne avant l'expansion.
- **Tu ne peux pas** instancier un composant défini dans un fichier inclus. `components` est strictement local au fichier qui le déclare, comme `config`. Dans les deux sens, c'est une erreur nommée, jamais une portée silencieusement fausse.

## Ce que ça coûte

**`--fix` refuse de réécrire un scénario qui utilise ces directives.** Les chemins de violation portent des index post-expansion ; une itération sur dix éléments décale de neuf tout ce qui suit, donc `--fix` patcherait le mauvais nœud. Il refuse plutôt que de corriger à côté — exactement comme pour `include`.

Tu peux toujours valider (`rustmotion validate` voit l'arbre expansé, donc la géométrie est vérifiée sur ce qui sera réellement rendu). Seule la réécriture automatique est indisponible.

## Erreurs nommées

Aucune de ces situations ne passe en silence :

| Situation | Diagnostic |
|---|---|
| Cycle entre composants | La chaîne complète (`a -> b -> a`), jamais un débordement de pile |
| `for-each` sur autre chose qu'un tableau | Ce qui a été trouvé à la place, avec un indice si ça ressemble à un `$var` non résolu |
| `use` d'un composant inconnu | Le nom manquant |
| Paramètre requis absent | Le nom du paramètre |
| Clé de `props` non déclarée | Le nom de la clé |
