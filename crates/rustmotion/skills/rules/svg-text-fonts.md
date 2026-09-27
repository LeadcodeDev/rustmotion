# `svg` : le `<text>` a besoin d'une police résolue

Un `<text>` dans un `svg` (`data` ou `src`) n'est jamais peint directement :
`usvg` le convertit en tracés de glyphes **au moment du parsing**, avant même
que `resvg` ne peigne quoi que ce soit. Cette conversion a besoin d'une police
présente dans sa base de polices — sans elle, l'élément est retiré de l'arbre
en silence, comme s'il n'avait jamais existé. Un `<rect>` ou un `<path>` du
même document continue de se peindre normalement : rien ne signale que le
texte, lui, a disparu.

## La base de polices système est chargée

Le composant construit ses `usvg::Options` avec une base de polices système
(`fontdb::Database::load_system_fonts()`, chargée une seule fois par
processus et partagée entre tous les nœuds `svg`), ce qui couvre tout
`font-family` correspondant à une police déjà installée sur la machine
(`Helvetica`, `Arial`, la police système par défaut…) — le même principe que
ce que `text` fait déjà via le `FontMgr` de Skia.

## Ce que ça ne couvre pas

Cette base de polices système est **distincte** du registre de polices
personnalisées de rustmotion (`fonts:` au niveau du scénario, source
`"google"` ou `path` local) : ce registre alimente le `FontMgr` de Skia pour
le composant `text`, pas le `fontdb` d'`usvg`. Un `<text>` de `svg` qui
référence une police déclarée dans `fonts:` mais absente du système ne
résout donc pas vers la bonne police — ce n'est pas un oubli d'implémentation
ponctuel, les deux composants s'appuient sur deux moteurs de police
complètement séparés.

Ça ne le fait *pas* disparaître pour autant : `usvg` ajoute toujours une
police générique (`serif`) en bout de la liste de recherche, donc tant que le
`fontdb` contient au moins une police quelconque, le texte se peint — avec la
police système par défaut à la place de celle demandée, pas dans le vide.
C'est un défaut visuel (mauvaise police), pas la disparition totale
qu'était le bug original.

Le cas encore silencieux est plus étroit : une machine de rendu sans **aucune**
police installée nulle part (un conteneur headless minimal, par exemple), où
même le repli générique n'a rien vers quoi se rabattre. Dans ce cas précis, le
composant ne reste plus silencieux : il compare le nombre de `<text>` présents
dans le SVG source au nombre de nœuds texte effectivement résolus par `usvg`,
et si l'un d'eux a été perdu, il écrit sur stderr :

```
Warning: svg: 1 of 1 <text> element(s) have no matching font face for their
font-family and will not be drawn. Declare the family in the scenario's
`fonts` list (a "google" source or a local `path`), or use a font already
installed on the system.
```

Le message ne se répète pas à chaque frame pendant le rendu : il est déduit
une seule fois par contenu SVG distinct.

## Contournement

Pour obtenir la police exacte demandée par un `<text>` de `svg` (pas un
repli), le contournement actuel est d'installer cette police sur la machine
de rendu, ou de convertir le texte en tracés avant de l'injecter dans le SVG
(export « outline » depuis l'éditeur vectoriel — le texte devient alors un
`<path>` ordinaire, qui n'a besoin d'aucune police). Faire résoudre `fonts:`
par `usvg` demanderait de faire transiter le registre de polices
personnalisées de `text` (`rustmotion-core::engine::renderer::fonts`) jusqu'au
`fontdb` du composant `svg` — en dehors du périmètre de ce correctif.

`usvg` journalise déjà en interne (`log::warn!`) chaque `font-family` sans
correspondance exacte avant de retomber sur le générique, mais rien dans le
binaire `rustmotion` n'installe de backend pour la crate `log` : ces traces
existent mais n'atteignent jamais stderr aujourd'hui.
