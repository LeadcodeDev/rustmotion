//! Data-driven repetition and reusable component templates.
//!
//! This is the answer to the dominant failure mode the original audit named:
//! an LLM asked for "ten identical cards, different data" hand-writes ten
//! JSON subtrees, and every copy is a chance to diverge (a forgotten color,
//! a stray `font-size`, a `position` that doesn't match its siblings). Two
//! directives close that gap, both usable inside any `children` array —
//! exactly where a component would go:
//!
//! - **`for-each`** repeats a `template` subtree once per element of an
//!   array, binding the current element's fields (plus `$index`) into it.
//! - **`use`** instantiates a named, reusable subtree declared once in a
//!   top-level `components` block, with `props` overrides — a factored-out
//!   component definition, the same relationship `include` has to a whole
//!   scenario file, but *within* one file and *without* the I/O.
//!
//! ## Why this lives in `rustmotion-core`, not `rustmotion`
//!
//! `include.rs` needs file/network I/O (`std::fs`, `ureq`), so it lives in
//! the `rustmotion` crate. This module is pure `serde_json::Value` rewriting
//! — no I/O, same as `variables.rs` — so it lives next to it here.
//!
//! ## Syntax, and why it looks like `include`/`config` rather than a third
//! dialect
//!
//! ```json
//! {
//!   "components": {
//!     "stat_card": {
//!       "params": {
//!         "label": { "type": "string" },
//!         "value": { "type": "number", "default": 0 },
//!         "color": { "type": "string", "default": "#6366F1" }
//!       },
//!       "template": {
//!         "type": "card",
//!         "style": { "width": "300px", "background": "$color" },
//!         "children": [
//!           { "type": "text", "content": "$label" },
//!           { "type": "counter", "value": "$value" }
//!         ]
//!       }
//!     }
//!   },
//!   "scenes": [{
//!     "duration": 3.0,
//!     "children": [
//!       {
//!         "for-each": "$rows",
//!         "template": { "use": "stat_card", "props": { "label": "$label", "value": "$value" } }
//!       }
//!     ]
//!   }]
//! }
//! ```
//!
//! `components[name].params` is deliberately the exact same shape as the
//! scenario-level `config` block (`{"type": ..., "default": ..., "description": ...}`,
//! see [`crate::schema::VariableDefinition`]) — a param is a variable scoped
//! to one component instead of the whole file. `use` + its overrides field
//! mirrors `IncludeDirective { include, config }` (a name plus overrides) —
//! *except* the overrides field is called **`props`**, not `config`. That is
//! a deliberate, load-bearing difference, not inconsistency: [`substitute`]
//! (shared with `variables.rs`) skips recursing into any object key literally
//! named `"config"`, so that the scenario-level `config` *declarations* block
//! (whose `default` values must stay literal, see
//! `variables::test_config_key_not_substituted`) is never accidentally
//! rewritten by whole-document substitution. Reusing that same key name for
//! `use`'s overrides would make a `for-each` binding (`$label`) placed inside
//! a *nested* `use`'s overrides silently never substitute — exactly the kind
//! of silent failure this workstream exists to remove. `props` sidesteps the
//! collision entirely while keeping the rest of the shape familiar.
//!
//! For `for-each`, each array element's own fields are bound directly (flat,
//! not `$item.label`): the codebase's existing `$name` substitution has no
//! dotted-path support (see `variables::parse_single_var_ref`), so an element
//! `{"label": "Revenue", "value": 120}` exposes `$label` and `$value`
//! straight into the template, exactly like a `config` default would. The
//! whole element is *also* bound to `$item` (for forwarding it wholesale,
//! e.g. into a nested `use`'s `props` via `{"$var": "item"}`), and the
//! 0-based position is bound to `$index`. Explicit data always wins: if an
//! element's own field is named `index` or `item`, that value is kept and the
//! built-in is not inserted over it.
//!
//! ## Pass ordering (load-bearing, tested in
//! `rustmotion/tests/templates_iteration.rs`)
//!
//! Every call site runs `expand_directives` immediately *after*
//! `variables::apply_variables` and *before* `Scenario` is deserialized —
//! same document, same pass boundary `include` sits on the other side of.
//! Concretely, per document (root scenario file, and independently for each
//! file pulled in by `include`, since `components` is file-local — see
//! below):
//!
//! 1. Parse JSON.
//! 2. `variables::apply_variables` — resolves the file's own `config`/`$var`.
//! 3. **`expand::expand_directives`** (this module) — resolves `for-each`/
//!    `use` using the now-literal document, then removes `components`.
//! 4. Deserialize into `Scenario`.
//! 5. `include::resolve_includes` — splices in child files (each of which
//!    already went through steps 1-4 independently inside
//!    `include::fetch_and_resolve`).
//!
//! Two consequences fall out of running expansion strictly after variable
//! substitution and strictly per-document:
//!
//! - **You *can* iterate over an array that came from a variable.**
//!   `"for-each": "$rows"` is, by the time this module sees it, no longer a
//!   `$`-string — step 2 already replaced it with the literal array (if
//!   `rows` is a declared `config` variable of array type). `for-each` itself
//!   never has to know variables exist.
//! - **You *cannot* instantiate a component defined in an included file** —
//!   not from the *parent's* `use` sites, anyway. `components` is scoped to
//!   the document it is declared in, the same way `config` is: each document
//!   gets its own `apply_variables` + `expand_directives` pass over its own
//!   text before it is ever spliced into anything else. A `use` inside a
//!   file that *includes* another file cannot see the includee's
//!   `components`, and a `use` inside the includee cannot see the includer's.
//!   This is a deliberate simplicity choice (no cross-file component
//!   registry, no import syntax to design and version) — see the module test
//!   `use_cannot_reach_a_component_defined_in_a_sibling_included_file` in
//!   `rustmotion/tests/templates_iteration.rs` for the resulting diagnostic.
//!
//! ## The index-shift trap (already drew blood once — see PR #145 / #160)
//!
//! `include` has the exact same shape of bug this module could reintroduce:
//! a directive that expands to a scene count other than 1 shifts every
//! later `views[V].scenes[S]` index, and `--fix` patches the raw JSON by
//! that same indexed path. `for-each` is strictly worse on this axis — ten
//! elements shift nine siblings, not (at most) a handful. This module does
//! **not** try to solve that by tracking pre/post-expansion index maps: it
//! solves it the way `include` already does, by removing the temptation.
//! `expand_directives` runs *before* `Scenario` is deserialized, so
//! `LoadedScenario::raw` (what `--fix` would serialize) is *already* the
//! expanded tree by the time `commands/validate.rs` sees it — same as
//! `include`'s resolved scenes are already spliced into `raw` by the time
//! `--fix` runs. `commands/validate.rs::refuse_fix` is extended with a
//! `UsesTemplateDirectives` case, detected the same (raw-substring,
//! conservative-by-design) way `UsesInclude` already is, so `--fix` refuses
//! outright rather than writing the expansion back over the author's
//! `for-each`/`use`/`components` source.

use std::collections::HashMap;

use serde::Deserialize;
use serde_json::Value;

use crate::error::{Result, RustmotionError};
use crate::schema::VariableType;
use crate::variables::substitute;

/// Defense-in-depth ceiling on nested `use`/`for-each` expansion. True
/// self-reference cycles are caught immediately by the name stack in
/// [`resolve_entry`] and never reach this; this only guards against
/// legitimately deep (non-cyclic) nesting run away, mirroring
/// `include::MAX_INCLUDE_DEPTH`'s role for the sibling mechanism.
const MAX_EXPANSION_DEPTH: u32 = 64;

/// One entry of the top-level `components` map: a named, parameterised
/// subtree. `params` reuses the exact shape of the scenario-level `config`
/// block, except a param's `default` is optional — omitting it makes the
/// parameter *required*, which `config` variables cannot express (every
/// `config` variable must have a default, since it is meant to render
/// standalone with no overrides at all; a component parameter has no such
/// obligation — an icon component's `icon` name, for instance, has no
/// sensible default).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentDefinition {
    #[serde(default)]
    params: HashMap<String, ComponentParam>,
    /// The subtree to instantiate: a single component object, or an array of
    /// sibling component objects (a fragment spliced in place).
    template: Value,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ComponentParam {
    #[serde(rename = "type")]
    #[allow(dead_code)]
    // documentation/schema parity with `config`; not cross-checked against `default`'s actual JSON type (same as `VariableDefinition::var_type` today)
    param_type: VariableType,
    #[serde(default)]
    default: Option<Value>,
    #[serde(default)]
    #[allow(dead_code)]
    description: Option<String>,
}

/// `{"use": "name", "props": {...}}` — instantiate a `components` entry.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct UseDirective {
    #[serde(rename = "use")]
    use_name: String,
    #[serde(default)]
    props: HashMap<String, Value>,
}

/// `{"for-each": [...], "template": {...}}` — repeat `template` once per
/// element of the (already variable-substituted) array.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ForEachDirective {
    #[serde(rename = "for-each")]
    for_each: Value,
    template: Value,
}

fn is_for_each(v: &Value) -> bool {
    matches!(v, Value::Object(m) if m.contains_key("for-each"))
}

fn is_use(v: &Value) -> bool {
    matches!(v, Value::Object(m) if m.contains_key("use"))
}

/// Expand every `for-each`/`use` directive found in any `children` array
/// anywhere in `value`, and consume the top-level `components` block (like
/// `variables::apply_variables` consumes `config`, it is removed so it never
/// reaches `Scenario`'s `deny_unknown_fields`). Call this once per document,
/// immediately after `variables::apply_variables` and before deserializing
/// into `Scenario` — see the module doc for why that ordering is load-bearing.
///
/// `file_label` is the same kind of label `apply_variables` takes (a file
/// path, `<inline>`, or `<root>`) — used only for error messages, alongside a
/// structural location built while walking (e.g. `scenes[2].children[1]`),
/// so a diagnostic names *where in the source* the offending directive is,
/// not just which file.
pub fn expand_directives(value: &mut Value, file_label: &str) -> Result<()> {
    let defs = extract_component_definitions(value, file_label)?;

    let Value::Object(root) = value else {
        return Ok(());
    };
    root.remove("components");

    if let Some(Value::Array(scenes)) = root.remove("scenes") {
        let mut out = Vec::with_capacity(scenes.len());
        for (i, mut scene) in scenes.into_iter().enumerate() {
            let scene_path = format!("scenes[{i}]");
            let mut stack = Vec::new();
            walk_children(&mut scene, &defs, file_label, &scene_path, &mut stack, 0)?;
            out.push(scene);
        }
        root.insert("scenes".to_string(), Value::Array(out));
    }

    if let Some(Value::Array(views)) = root.remove("composition") {
        let mut out_views = Vec::with_capacity(views.len());
        for (vi, mut view) in views.into_iter().enumerate() {
            if let Value::Object(vmap) = &mut view {
                if let Some(Value::Array(scenes)) = vmap.remove("scenes") {
                    let mut out = Vec::with_capacity(scenes.len());
                    for (si, mut scene) in scenes.into_iter().enumerate() {
                        let scene_path = format!("composition[{vi}].scenes[{si}]");
                        let mut stack = Vec::new();
                        walk_children(&mut scene, &defs, file_label, &scene_path, &mut stack, 0)?;
                        out.push(scene);
                    }
                    vmap.insert("scenes".to_string(), Value::Array(out));
                }
            }
            out_views.push(view);
        }
        root.insert("composition".to_string(), Value::Array(out_views));
    }

    warn_unresolved_after_expansion(value, file_label);
    Ok(())
}

/// Report `$name`s that survived both variable substitution and directive
/// expansion.
///
/// `variables::apply_variables` runs its own scan, but *before* this pass and
/// skipping `template`/`props`/`components` — every `$name` in there is a
/// binding this function is about to resolve, and reporting them would emit a
/// warning per binding on every correct scenario. Those keys are consumed by
/// the time we get here, so scanning the expanded document sees only genuine
/// leftovers: a `$typo` in a template that matched no data field, or a `$` in
/// ordinary content.
///
/// A warning rather than an error, matching what `apply_variables` decided for
/// the same diagnostic: a literal `$` in a price or a shell path is legitimate
/// content and must not fail a render.
fn warn_unresolved_after_expansion(value: &Value, file_label: &str) {
    for name in crate::variables::find_unresolved(value) {
        eprintln!(
            "Warning: {}",
            crate::error::RustmotionError::UnresolvedVariable {
                name,
                path: file_label.to_string(),
            }
        );
    }
}

fn extract_component_definitions(
    value: &Value,
    file_label: &str,
) -> Result<HashMap<String, ComponentDefinition>> {
    let Value::Object(root) = value else {
        return Ok(HashMap::new());
    };
    match root.get("components") {
        None => Ok(HashMap::new()),
        Some(Value::Object(defs_map)) => {
            let mut out = HashMap::with_capacity(defs_map.len());
            for (name, def_val) in defs_map {
                let def: ComponentDefinition =
                    serde_json::from_value(def_val.clone()).map_err(|e| {
                        RustmotionError::ComponentDefinitionInvalid {
                            name: name.clone(),
                            path: file_label.to_string(),
                            reason: e.to_string(),
                        }
                    })?;
                out.insert(name.clone(), def);
            }
            Ok(out)
        }
        Some(_) => Err(RustmotionError::ComponentsBlockNotObject {
            path: file_label.to_string(),
        }),
    }
}

/// Find the `children` array on `value` (if any), expand every entry in it
/// (concrete entries pass through unchanged but are still recursed into, so
/// nested containers get their own `children` expanded too), then recurse
/// into every other field generically — a `for-each`/`use` can appear
/// anywhere a `children` array can, at any nesting depth.
fn walk_children(
    value: &mut Value,
    defs: &HashMap<String, ComponentDefinition>,
    file_label: &str,
    location: &str,
    stack: &mut Vec<String>,
    depth: u32,
) -> Result<()> {
    match value {
        Value::Object(map) => {
            if matches!(map.get("children"), Some(Value::Array(_))) {
                if let Some(Value::Array(arr)) = map.remove("children") {
                    let mut expanded = Vec::with_capacity(arr.len());
                    for (i, entry) in arr.into_iter().enumerate() {
                        let entry_loc = format!("{location}.children[{i}]");
                        expanded.extend(resolve_entry(
                            entry, defs, file_label, &entry_loc, stack, depth,
                        )?);
                    }
                    map.insert("children".to_string(), Value::Array(expanded));
                }
            }
            for (k, v) in map.iter_mut() {
                if k == "children" {
                    continue; // already fully expanded above
                }
                walk_children(v, defs, file_label, location, stack, depth)?;
            }
        }
        Value::Array(arr) => {
            for v in arr.iter_mut() {
                walk_children(v, defs, file_label, location, stack, depth)?;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Resolve one `children` array entry into zero or more concrete entries.
/// A plain component entry resolves to exactly itself (after recursing into
/// its own `children`, if it has one). A `for-each`/`use` directive resolves
/// to the nodes it produces — which are, in turn, run back through this same
/// function, so a `for-each` template that is itself a `use`, or a `use`
/// whose template is itself a `for-each`, composes without special-casing.
///
/// A bare JSON array (a `for-each`/`use` template written as a *fragment* —
/// several sibling nodes instead of one) is flattened here too, generically,
/// rather than only where `use` happens to produce one: both directives'
/// `template` accept either shape, and this is the single place that
/// splices a fragment's elements into the parent `children` array instead of
/// nesting a raw `[...]` inside it (which downstream `Component`
/// deserialization has no concept of).
fn resolve_entry(
    entry: Value,
    defs: &HashMap<String, ComponentDefinition>,
    file_label: &str,
    location: &str,
    stack: &mut Vec<String>,
    depth: u32,
) -> Result<Vec<Value>> {
    if depth > MAX_EXPANSION_DEPTH {
        return Err(RustmotionError::ExpansionDepthExceeded {
            limit: MAX_EXPANSION_DEPTH,
            path: format!("{file_label}: {location}"),
        });
    }

    if let Value::Array(fragment) = entry {
        let mut out = Vec::with_capacity(fragment.len());
        for (i, n) in fragment.into_iter().enumerate() {
            let frag_loc = format!("{location}[{i}]");
            out.extend(resolve_entry(
                n,
                defs,
                file_label,
                &frag_loc,
                stack,
                depth + 1,
            )?);
        }
        return Ok(out);
    }

    if is_for_each(&entry) {
        let produced = expand_for_each_directive(entry, file_label, location)?;
        let mut out = Vec::with_capacity(produced.len());
        for (i, node) in produced.into_iter().enumerate() {
            let iter_loc = format!("{location}[{i}]");
            out.extend(resolve_entry(
                node,
                defs,
                file_label,
                &iter_loc,
                stack,
                depth + 1,
            )?);
        }
        return Ok(out);
    }

    if is_use(&entry) {
        let (name, node) = expand_use_directive(entry, defs, file_label, location)?;
        if stack.contains(&name) {
            let mut chain = stack.clone();
            chain.push(name);
            return Err(RustmotionError::ComponentCycle {
                chain: chain.join(" -> "),
                path: format!("{file_label}: {location}"),
            });
        }
        stack.push(name);
        let result = resolve_entry(node, defs, file_label, location, stack, depth + 1);
        stack.pop();
        return result;
    }

    let mut node = entry;
    walk_children(&mut node, defs, file_label, location, stack, depth)?;
    Ok(vec![node])
}

fn expand_for_each_directive(entry: Value, file_label: &str, location: &str) -> Result<Vec<Value>> {
    let directive: ForEachDirective =
        serde_json::from_value(entry).map_err(|e| RustmotionError::ForEachDirectiveInvalid {
            path: format!("{file_label}: {location}"),
            reason: e.to_string(),
        })?;

    let items = match &directive.for_each {
        Value::Array(items) => items.clone(),
        other => {
            return Err(RustmotionError::ForEachNotArray {
                path: format!("{file_label}: {location}"),
                found: describe_value(other),
            })
        }
    };

    let mut out = Vec::with_capacity(items.len());
    for (idx, element) in items.into_iter().enumerate() {
        let mut bindings: HashMap<String, Value> = HashMap::new();
        if let Value::Object(obj) = &element {
            for (k, v) in obj {
                bindings.insert(k.clone(), v.clone());
            }
        }
        // Explicit data wins: only fill these in if the element didn't
        // already define a field with that name.
        bindings
            .entry("index".to_string())
            .or_insert_with(|| Value::from(idx));
        bindings
            .entry("item".to_string())
            .or_insert_with(|| element.clone());

        let mut node = directive.template.clone();
        substitute(&mut node, &bindings, file_label)?;
        out.push(node);
    }
    Ok(out)
}

fn expand_use_directive(
    entry: Value,
    defs: &HashMap<String, ComponentDefinition>,
    file_label: &str,
    location: &str,
) -> Result<(String, Value)> {
    let directive: UseDirective =
        serde_json::from_value(entry).map_err(|e| RustmotionError::UseDirectiveInvalid {
            path: format!("{file_label}: {location}"),
            reason: e.to_string(),
        })?;

    let def = defs
        .get(&directive.use_name)
        .ok_or_else(|| RustmotionError::UnknownComponent {
            name: directive.use_name.clone(),
            path: format!("{file_label}: {location}"),
        })?;

    for key in directive.props.keys() {
        if !def.params.contains_key(key) {
            return Err(RustmotionError::UnknownComponentParam {
                component: directive.use_name.clone(),
                param: key.clone(),
                path: format!("{file_label}: {location}"),
            });
        }
    }

    let mut bindings: HashMap<String, Value> = HashMap::with_capacity(def.params.len());
    for (pname, pdef) in &def.params {
        match directive.props.get(pname) {
            Some(v) => {
                bindings.insert(pname.clone(), v.clone());
            }
            None => match &pdef.default {
                Some(d) => {
                    bindings.insert(pname.clone(), d.clone());
                }
                None => {
                    return Err(RustmotionError::ComponentParamMissing {
                        component: directive.use_name.clone(),
                        param: pname.clone(),
                        path: format!("{file_label}: {location}"),
                    })
                }
            },
        }
    }

    let mut node = def.template.clone();
    substitute(&mut node, &bindings, file_label)?;
    Ok((directive.use_name.clone(), node))
}

fn describe_value(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(b) => format!("boolean ({b})"),
        Value::Number(n) => format!("number ({n})"),
        Value::String(s) => {
            let preview: String = s.chars().take(40).collect();
            let ellipsis = if s.chars().count() > 40 { "…" } else { "" };
            format!(
                "string (\"{preview}{ellipsis}\"){}",
                if s.starts_with('$') {
                    " — looks like an unresolved/undeclared variable reference"
                } else {
                    ""
                }
            )
        }
        Value::Object(_) => "object".to_string(),
        Value::Array(_) => "array".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn expand(mut value: Value) -> Result<Value> {
        expand_directives(&mut value, "test.json")?;
        Ok(value)
    }

    // ---- unresolved-reference scanning across the two passes ----

    /// `apply_variables` scans for leftover `$name`s before this pass runs.
    /// Left unguarded it reported every template binding as a typo — six
    /// warnings on the canonical example, each accusing the author of a
    /// mistake they had not made. Warnings that are reliably wrong teach the
    /// reader to ignore warnings, which costs more than the scan is worth.
    #[test]
    fn template_bindings_are_not_reported_as_unresolved_before_expansion() {
        let doc = json!({
            "components": {
                "card": {
                    "params": { "label": { "type": "string" } },
                    "template": { "type": "text", "content": "$label" }
                }
            },
            "scenes": [{ "duration": 1.0, "children": [{
                "for-each": [{ "label": "one" }],
                "template": { "use": "card", "props": { "label": "$label" } }
            }]}]
        });
        assert!(
            crate::variables::find_unresolved(&doc).is_empty(),
            "bindings inside components/template/props belong to expansion, \
             not to the pre-expansion scan: {:?}",
            crate::variables::find_unresolved(&doc)
        );
    }

    /// The other half: skipping those keys must not turn a false positive
    /// into a false negative. Once expansion has consumed them, a `$name`
    /// that matched no data field is a genuine leftover and is visible to the
    /// very same scan.
    #[test]
    fn a_typo_inside_a_template_is_still_found_after_expansion() {
        let expanded = expand(json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{ "duration": 1.0, "children": [{
                "for-each": [{ "label": "one" }],
                "template": { "type": "text", "content": "$labl" }
            }]}]
        }))
        .expect("a typo is a warning, not a hard error");
        assert_eq!(
            crate::variables::find_unresolved(&expanded),
            vec!["labl".to_string()],
            "the leftover must be visible once template/props are gone"
        );
    }

    /// And the correct spelling leaves nothing behind, so the scan above is
    /// discriminating rather than merely quiet.
    #[test]
    fn a_correct_binding_leaves_nothing_unresolved_after_expansion() {
        let expanded = expand(json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{ "duration": 1.0, "children": [{
                "for-each": [{ "label": "one" }],
                "template": { "type": "text", "content": "$label" }
            }]}]
        }))
        .expect("expands");
        assert!(crate::variables::find_unresolved(&expanded).is_empty());
    }

    // ---- for-each ----

    #[test]
    fn for_each_repeats_template_once_per_element_binding_its_fields() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": [
                        { "label": "Revenue", "value": 120 },
                        { "label": "Users", "value": 340 }
                    ],
                    "template": { "type": "text", "content": "$label: $value" }
                }]
            }]
        });
        let out = expand(doc).unwrap();
        let children = out["scenes"][0]["children"].as_array().unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0]["content"], json!("Revenue: 120"));
        assert_eq!(children[1]["content"], json!("Users: 340"));
    }

    #[test]
    fn for_each_binds_index_and_whole_item() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": ["a", "b", "c"],
                    "template": { "type": "text", "content": "$index:$item" }
                }]
            }]
        });
        let out = expand(doc).unwrap();
        let children = out["scenes"][0]["children"].as_array().unwrap();
        assert_eq!(children.len(), 3);
        assert_eq!(children[0]["content"], json!("0:a"));
        assert_eq!(children[1]["content"], json!("1:b"));
        assert_eq!(children[2]["content"], json!("2:c"));
    }

    #[test]
    fn for_each_lets_explicit_item_fields_win_over_built_in_index() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": [{ "index": "custom", "label": "x" }],
                    "template": { "type": "text", "content": "$index" }
                }]
            }]
        });
        let out = expand(doc).unwrap();
        assert_eq!(out["scenes"][0]["children"][0]["content"], json!("custom"));
    }

    #[test]
    fn for_each_over_empty_array_produces_nothing_and_is_not_an_error() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": [],
                    "template": { "type": "text", "content": "unused" }
                }]
            }]
        });
        let out = expand(doc).unwrap();
        assert_eq!(out["scenes"][0]["children"], json!([]));
    }

    #[test]
    fn for_each_source_that_is_not_an_array_is_a_named_error_not_a_silent_empty_result() {
        // The exact silent failure mode the brief calls out: a for-each
        // source key typo'd or referencing an undeclared variable leaves a
        // literal, non-array `$...` string here — this must be a hard error
        // naming where, not a quietly empty `children`.
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": "$itms",
                    "template": { "type": "text", "content": "$label" }
                }]
            }]
        });
        let err = expand(doc).expect_err("non-array for-each source must fail loudly");
        assert!(
            matches!(err, RustmotionError::ForEachNotArray { .. }),
            "{err}"
        );
        let msg = err.to_string();
        assert!(msg.contains("scenes[0].children[0]"), "{msg}");
        assert!(msg.contains("unresolved"), "{msg}");
    }

    #[test]
    fn for_each_missing_template_is_a_named_error() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{ "for-each": [1, 2, 3] }]
            }]
        });
        let err = expand(doc).expect_err("missing template must fail");
        assert!(
            matches!(err, RustmotionError::ForEachDirectiveInvalid { .. }),
            "{err}"
        );
    }

    #[test]
    fn for_each_with_a_fragment_template_splices_every_sibling_in_place_not_a_nested_array() {
        // Each iteration's `template` is an *array* of two sibling nodes
        // (an icon + a label), not a single object — both must end up as
        // direct, flat siblings in the surrounding `children` array; a
        // nested `[...]` there would not deserialize as a component.
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": [ { "label": "A" }, { "label": "B" } ],
                    "template": [
                        { "type": "icon", "icon": "lucide:dot" },
                        { "type": "text", "content": "$label" }
                    ]
                }]
            }]
        });
        let out = expand(doc).unwrap();
        let children = out["scenes"][0]["children"].as_array().unwrap();
        assert_eq!(
            children.len(),
            4,
            "2 iterations x 2 fragment nodes = 4 flat siblings, got: {children:#?}"
        );
        assert!(children.iter().all(|c| c.is_object()), "{children:#?}");
        assert_eq!(children[0]["type"], json!("icon"));
        assert_eq!(children[1]["content"], json!("A"));
        assert_eq!(children[2]["type"], json!("icon"));
        assert_eq!(children[3]["content"], json!("B"));
    }

    // ---- use / components ----

    fn doc_with_stat_card(props: Value) -> Value {
        json!({
            "video": { "width": 100, "height": 100 },
            "components": {
                "stat_card": {
                    "params": {
                        "label": { "type": "string" },
                        "value": { "type": "number", "default": 0 },
                        "color": { "type": "string", "default": "#6366F1" }
                    },
                    "template": {
                        "type": "card",
                        "style": { "background": "$color" },
                        "children": [
                            { "type": "text", "content": "$label" },
                            { "type": "counter", "value": "$value" }
                        ]
                    }
                }
            },
            "scenes": [{
                "duration": 1.0,
                "children": [{ "use": "stat_card", "props": props }]
            }]
        })
    }

    #[test]
    fn use_instantiates_a_component_with_props_overriding_defaults() {
        let out = expand(doc_with_stat_card(
            json!({ "label": "Revenue", "value": 42 }),
        ))
        .unwrap();
        let card = &out["scenes"][0]["children"][0];
        assert_eq!(card["type"], json!("card"));
        assert_eq!(card["style"]["background"], json!("#6366F1"));
        assert_eq!(card["children"][0]["content"], json!("Revenue"));
        assert_eq!(card["children"][1]["value"], json!(42));
    }

    #[test]
    fn use_falls_back_to_param_default_when_not_overridden() {
        let out = expand(doc_with_stat_card(json!({ "label": "Users" }))).unwrap();
        assert_eq!(
            out["scenes"][0]["children"][0]["children"][1]["value"],
            json!(0)
        );
    }

    #[test]
    fn components_block_does_not_survive_expansion() {
        let out = expand(doc_with_stat_card(json!({ "label": "x" }))).unwrap();
        assert!(out.get("components").is_none());
    }

    #[test]
    fn use_of_unknown_component_is_a_named_error() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{ "use": "does_not_exist", "props": {} }]
            }]
        });
        let err = expand(doc).expect_err("unknown component must fail");
        match &err {
            RustmotionError::UnknownComponent { name, path } => {
                assert_eq!(name, "does_not_exist");
                assert!(path.contains("scenes[0].children[0]"), "{path}");
            }
            other => panic!("expected UnknownComponent, got {other}"),
        }
    }

    #[test]
    fn use_missing_a_required_parameter_is_a_named_error() {
        // `label` has no default in `doc_with_stat_card` — omitting it must
        // fail, not silently render an empty/placeholder value.
        let out = expand(doc_with_stat_card(json!({})));
        let err = out.expect_err("missing required param must fail");
        match &err {
            RustmotionError::ComponentParamMissing {
                component, param, ..
            } => {
                assert_eq!(component, "stat_card");
                assert_eq!(param, "label");
            }
            other => panic!("expected ComponentParamMissing, got {other}"),
        }
    }

    #[test]
    fn use_with_an_undeclared_prop_key_is_a_named_error() {
        let out = expand(doc_with_stat_card(
            json!({ "label": "x", "labell": "typo" }),
        ));
        let err = out.expect_err("typo'd prop key must fail");
        match &err {
            RustmotionError::UnknownComponentParam { param, .. } => assert_eq!(param, "labell"),
            other => panic!("expected UnknownComponentParam, got {other}"),
        }
    }

    #[test]
    fn use_of_a_component_that_uses_itself_is_a_named_cycle_not_a_stack_overflow() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "components": {
                "recursive": {
                    "params": {},
                    "template": { "type": "card", "children": [ { "use": "recursive", "props": {} } ] }
                }
            },
            "scenes": [{
                "duration": 1.0,
                "children": [{ "use": "recursive", "props": {} }]
            }]
        });
        let err = expand(doc).expect_err("self-referencing component must fail");
        match &err {
            RustmotionError::ComponentCycle { chain, .. } => {
                assert!(chain.contains("recursive"), "{chain}");
            }
            other => panic!("expected ComponentCycle, got {other}"),
        }
    }

    #[test]
    fn indirect_two_hop_cycle_is_also_a_named_cycle() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "components": {
                "a": { "params": {}, "template": { "type": "card", "children": [ { "use": "b", "props": {} } ] } },
                "b": { "params": {}, "template": { "type": "card", "children": [ { "use": "a", "props": {} } ] } }
            },
            "scenes": [{
                "duration": 1.0,
                "children": [{ "use": "a", "props": {} }]
            }]
        });
        let err = expand(doc).expect_err("indirect cycle must fail");
        match &err {
            RustmotionError::ComponentCycle { chain, .. } => {
                assert!(chain.contains('a') && chain.contains('b'), "{chain}");
            }
            other => panic!("expected ComponentCycle, got {other}"),
        }
    }

    #[test]
    fn use_with_a_fragment_template_splices_every_sibling_in_place() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "components": {
                "icon_label": {
                    "params": { "label": { "type": "string" } },
                    "template": [
                        { "type": "icon", "icon": "lucide:dot" },
                        { "type": "text", "content": "$label" }
                    ]
                }
            },
            "scenes": [{
                "duration": 1.0,
                "children": [{ "use": "icon_label", "props": { "label": "hi" } }]
            }]
        });
        let out = expand(doc).unwrap();
        let children = out["scenes"][0]["children"].as_array().unwrap();
        assert_eq!(children.len(), 2, "{children:#?}");
        assert_eq!(children[0]["type"], json!("icon"));
        assert_eq!(children[1]["content"], json!("hi"));
    }

    // ---- composition of the two directives ----

    #[test]
    fn for_each_template_can_be_a_use_directive() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "components": {
                "row": {
                    "params": { "label": { "type": "string" } },
                    "template": { "type": "text", "content": "$label" }
                }
            },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": [ { "label": "A" }, { "label": "B" } ],
                    "template": { "use": "row", "props": { "label": "$label" } }
                }]
            }]
        });
        let out = expand(doc).unwrap();
        let children = out["scenes"][0]["children"].as_array().unwrap();
        assert_eq!(children.len(), 2);
        assert_eq!(children[0]["content"], json!("A"));
        assert_eq!(children[1]["content"], json!("B"));
    }

    #[test]
    fn use_template_can_contain_a_nested_for_each() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "components": {
                "list_card": {
                    "params": { "items": { "type": "array" } },
                    "template": {
                        "type": "card",
                        "children": [{
                            "for-each": "$items",
                            "template": { "type": "text", "content": "$item" }
                        }]
                    }
                }
            },
            "scenes": [{
                "duration": 1.0,
                "children": [{ "use": "list_card", "props": { "items": ["x", "y", "z"] } }]
            }]
        });
        let out = expand(doc).unwrap();
        let inner = out["scenes"][0]["children"][0]["children"]
            .as_array()
            .unwrap();
        assert_eq!(inner.len(), 3);
        assert_eq!(inner[2]["content"], json!("z"));
    }

    #[test]
    fn nested_children_containers_are_expanded_recursively() {
        let doc = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "type": "card",
                    "children": [{
                        "for-each": [{ "v": 1 }, { "v": 2 }],
                        "template": { "type": "text", "content": "$v" }
                    }]
                }]
            }]
        });
        let out = expand(doc).unwrap();
        let inner = out["scenes"][0]["children"][0]["children"]
            .as_array()
            .unwrap();
        assert_eq!(inner.len(), 2);
        assert_eq!(inner[0]["content"], json!(1));
        assert_eq!(inner[1]["content"], json!(2));
    }

    // ---- the tree-identity proof, at the JSON-value level ----

    #[test]
    fn for_each_authored_tree_is_identical_to_the_hand_written_equivalent() {
        let generated = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [{
                    "for-each": [
                        { "label": "Revenue", "value": 120 },
                        { "label": "Users", "value": 340 },
                        { "label": "Growth", "value": 8 }
                    ],
                    "template": {
                        "type": "card",
                        "style": { "width": "200px" },
                        "children": [
                            { "type": "text", "content": "$label" },
                            { "type": "counter", "value": "$value" }
                        ]
                    }
                }]
            }]
        });

        let hand_written = json!({
            "video": { "width": 100, "height": 100 },
            "scenes": [{
                "duration": 1.0,
                "children": [
                    { "type": "card", "style": { "width": "200px" }, "children": [
                        { "type": "text", "content": "Revenue" },
                        { "type": "counter", "value": 120 }
                    ]},
                    { "type": "card", "style": { "width": "200px" }, "children": [
                        { "type": "text", "content": "Users" },
                        { "type": "counter", "value": 340 }
                    ]},
                    { "type": "card", "style": { "width": "200px" }, "children": [
                        { "type": "text", "content": "Growth" },
                        { "type": "counter", "value": 8 }
                    ]}
                ]
            }]
        });

        let expanded = expand(generated).unwrap();
        assert_eq!(
            expanded, hand_written,
            "the for-each-authored tree must be byte-for-byte identical (as JSON values) to the \
             hand-written equivalent — this is the only proof that factoring changes nothing about \
             what gets rendered"
        );
    }
}
