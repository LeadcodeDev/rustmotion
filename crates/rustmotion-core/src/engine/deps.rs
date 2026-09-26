use std::collections::{HashMap, HashSet, VecDeque};

use crate::engine::box_tree::{BoxKind, BoxNode};
use crate::engine::layout_pass::{BoxLayout, LayoutResult};
use crate::engine::paint_pass::animated_transform;
use crate::engine::renderer::GlyphMetric;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeRef {
    pub id: String,
    pub prop: String,
}

pub fn scan_node_refs(src: &str) -> Vec<NodeRef> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i + 4 <= bytes.len() {
        let is_word_start = i == 0 || !is_ident_byte(bytes[i - 1]);
        if is_word_start && &bytes[i..i + 4] == b"node" {
            let after_ident = i + 4;
            let is_word_end = bytes.get(after_ident).is_none_or(|&b| !is_ident_byte(b));
            if is_word_end {
                if let Some((node_ref, next)) = try_parse_node_call(src, after_ident) {
                    out.push(node_ref);
                    i = next;
                    continue;
                }
            }
        }
        i += 1;
    }
    out
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn skip_ws(bytes: &[u8], mut i: usize) -> usize {
    while i < bytes.len() && (bytes[i] as char).is_whitespace() {
        i += 1;
    }
    i
}

fn try_parse_node_call(src: &str, after_ident: usize) -> Option<(NodeRef, usize)> {
    let bytes = src.as_bytes();
    let mut i = skip_ws(bytes, after_ident);
    if bytes.get(i) != Some(&b'(') {
        return None;
    }
    i = skip_ws(bytes, i + 1);
    let (id, next) = scan_string_literal(src, i)?;
    i = skip_ws(bytes, next);
    if bytes.get(i) != Some(&b',') {
        return None;
    }
    i = skip_ws(bytes, i + 1);
    let (prop, next) = scan_string_literal(src, i)?;
    i = skip_ws(bytes, next);
    if bytes.get(i) != Some(&b')') {
        return None;
    }
    Some((NodeRef { id, prop }, i + 1))
}

fn scan_string_literal(src: &str, start: usize) -> Option<(String, usize)> {
    let bytes = src.as_bytes();
    if bytes.get(start) != Some(&b'"') {
        return None;
    }
    let mut j = start + 1;
    let mut s = String::new();
    loop {
        match bytes.get(j) {
            None => return None,
            Some(b'"') => return Some((s, j + 1)),
            Some(b'\\') if bytes.get(j + 1) == Some(&b'"') => {
                s.push('"');
                j += 2;
            }
            Some(b'\\') if bytes.get(j + 1) == Some(&b'\\') => {
                s.push('\\');
                j += 2;
            }
            Some(_) => {
                let ch = src[j..].chars().next()?;
                s.push(ch);
                j += ch.len_utf8();
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DepsError {
    #[error("node(\"...\") dependency cycle: {}", .chain.join(" -> "))]
    Cycle { chain: Vec<String> },
    #[error(
        "node \"{referencing}\" references unknown id \"{target}\" via node(\"{target}\", ...)"
    )]
    UnknownId { referencing: String, target: String },
    #[error(
        "node \"{referencing}\" references \"{target}\" from another scene \
         (reference_cross_scene: scenes are independent render units)"
    )]
    CrossScene { referencing: String, target: String },
    #[error("duplicate node id \"{0}\" declared more than once in the same scene")]
    DuplicateId(String),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DepGraph {
    order: Vec<String>,
}

impl DepGraph {
    pub fn order(&self) -> &[String] {
        &self.order
    }

    pub fn build(
        nodes: &[(String, Vec<NodeRef>)],
        other_scene_ids: &HashSet<String>,
    ) -> Result<DepGraph, DepsError> {
        let mut known: HashSet<&str> = HashSet::with_capacity(nodes.len());
        for (id, _) in nodes {
            if !known.insert(id.as_str()) {
                return Err(DepsError::DuplicateId(id.clone()));
            }
        }

        let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
        let mut indegree: HashMap<&str, usize> =
            nodes.iter().map(|(id, _)| (id.as_str(), 0)).collect();

        for (id, refs) in nodes {
            for r in refs {
                if !known.contains(r.id.as_str()) {
                    if other_scene_ids.contains(&r.id) {
                        return Err(DepsError::CrossScene {
                            referencing: id.clone(),
                            target: r.id.clone(),
                        });
                    }
                    return Err(DepsError::UnknownId {
                        referencing: id.clone(),
                        target: r.id.clone(),
                    });
                }
                dependents
                    .entry(r.id.as_str())
                    .or_default()
                    .push(id.as_str());
                *indegree.get_mut(id.as_str()).expect("id is known") += 1;
            }
        }

        let mut queue: VecDeque<&str> = nodes
            .iter()
            .map(|(id, _)| id.as_str())
            .filter(|id| indegree[id] == 0)
            .collect();
        let mut order: Vec<String> = Vec::with_capacity(nodes.len());
        while let Some(id) = queue.pop_front() {
            order.push(id.to_string());
            if let Some(deps) = dependents.get(id) {
                for &dep in deps {
                    let e = indegree.get_mut(dep).expect("dependent is known");
                    *e -= 1;
                    if *e == 0 {
                        queue.push_back(dep);
                    }
                }
            }
        }

        if order.len() != nodes.len() {
            let resolved: HashSet<&str> = order.iter().map(|s| s.as_str()).collect();
            let residual: Vec<&str> = nodes
                .iter()
                .map(|(id, _)| id.as_str())
                .filter(|id| !resolved.contains(id))
                .collect();
            return Err(DepsError::Cycle {
                chain: find_cycle_chain(&residual, nodes),
            });
        }

        Ok(DepGraph { order })
    }
}

fn find_cycle_chain(residual: &[&str], nodes: &[(String, Vec<NodeRef>)]) -> Vec<String> {
    let deps: HashMap<&str, Vec<&str>> = nodes
        .iter()
        .map(|(id, refs)| (id.as_str(), refs.iter().map(|r| r.id.as_str()).collect()))
        .collect();
    let residual_set: HashSet<&str> = residual.iter().copied().collect();

    let start = residual[0];
    let mut path: Vec<&str> = vec![start];
    let mut current = start;
    loop {
        let next = deps
            .get(current)
            .into_iter()
            .flatten()
            .find(|n| residual_set.contains(*n))
            .copied();
        match next {
            Some(n) => {
                if let Some(start_idx) = path.iter().position(|&x| x == n) {
                    let mut chain: Vec<String> =
                        path[start_idx..].iter().map(|s| s.to_string()).collect();
                    chain.push(n.to_string());
                    return chain;
                }
                path.push(n);
                current = n;
            }
            None => {
                return path.iter().map(|s| s.to_string()).collect();
            }
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TextMetrics {
    pub text_width: f32,
    pub cap_height: f32,
    pub ascender: f32,
    pub baseline: f32,
    pub glyphs: Vec<GlyphMetric>,
}

pub trait TextMetricsProvider {
    fn text_metrics(
        &self,
        payload: &(dyn std::any::Any + Send + Sync),
        content_box_width: f32,
    ) -> Option<TextMetrics>;
}

pub struct NoTextMetrics;

impl TextMetricsProvider for NoTextMetrics {
    fn text_metrics(
        &self,
        _payload: &(dyn std::any::Any + Send + Sync),
        _content_box_width: f32,
    ) -> Option<TextMetrics> {
        None
    }
}

#[derive(Debug, Clone, Copy, Default)]
struct Transform {
    tx: f32,
    ty: f32,
    scale: f32,
    rotation: f32,
    opacity: f32,
}

#[derive(Debug, Clone)]
pub struct ResolvedNode {
    layout: BoxLayout,
    transform: Transform,
    text: Option<TextMetrics>,
}

impl ResolvedNode {
    pub fn new(
        layout: BoxLayout,
        transform: (f32, f32, f32, f32, f32),
        text: Option<TextMetrics>,
    ) -> Self {
        let (tx, ty, scale, rotation, opacity) = transform;
        Self {
            layout,
            transform: Transform {
                tx,
                ty,
                scale,
                rotation,
                opacity,
            },
            text,
        }
    }

    pub fn prop(&self, prop: &str) -> Option<f64> {
        match prop {
            "x" => Some(self.layout.x as f64),
            "y" => Some(self.layout.y as f64),
            "width" => Some(self.layout.width as f64),
            "height" => Some(self.layout.height as f64),
            "cx" => Some(self.layout.cx() as f64),
            "cy" => Some(self.layout.cy() as f64),
            "right" => Some(self.layout.right() as f64),
            "bottom" => Some(self.layout.bottom() as f64),
            "tx" => Some(self.transform.tx as f64),
            "ty" => Some(self.transform.ty as f64),
            "scale" => Some(self.transform.scale as f64),
            "rotation" => Some(self.transform.rotation as f64),
            "opacity" => Some(self.transform.opacity as f64),
            "textWidth" => self.text.as_ref().map(|t| t.text_width as f64),
            "capHeight" => self.text.as_ref().map(|t| t.cap_height as f64),
            "ascender" => self.text.as_ref().map(|t| t.ascender as f64),
            "baseline" => self.text.as_ref().map(|t| t.baseline as f64),
            "glyph_count" => self.text.as_ref().map(|t| t.glyphs.len() as f64),
            _ => glyph_prop(self.text.as_ref(), prop),
        }
    }
}

fn glyph_prop(text: Option<&TextMetrics>, prop: &str) -> Option<f64> {
    let text = text?;
    if let Some(n) = prop.strip_prefix("glyph_x:") {
        let idx: usize = n.parse().ok()?;
        return text.glyphs.get(idx).map(|g| g.x as f64);
    }
    if let Some(n) = prop.strip_prefix("glyph_cx:") {
        let idx: usize = n.parse().ok()?;
        return text.glyphs.get(idx).map(|g| (g.x + g.width / 2.0) as f64);
    }
    None
}

pub fn snapshot_node(
    node: &BoxNode,
    layout: &LayoutResult,
    viewport: (f32, f32),
    text_provider: &dyn TextMetricsProvider,
) -> Option<ResolvedNode> {
    let box_layout = *layout.get(node.id)?;
    let transform = animated_transform(&node.css, &box_layout, viewport);
    let text = match &node.kind {
        BoxKind::Component(payload) | BoxKind::Ghost(payload) => {
            let (_, _, content_w, _) = box_layout.content_box();
            text_provider.text_metrics(payload.as_ref(), content_w)
        }
        BoxKind::Container => None,
    };
    Some(ResolvedNode::new(box_layout, transform, text))
}

#[derive(Debug, Clone, Default)]
pub struct ResolvedFrame {
    nodes: HashMap<String, ResolvedNode>,
}

impl ResolvedFrame {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, id: impl Into<String>, node: ResolvedNode) {
        self.nodes.insert(id.into(), node);
    }

    pub fn node_prop(&self, id: &str, prop: &str) -> Option<f64> {
        self.nodes.get(id)?.prop(prop)
    }
}

pub struct FrameScope<'a>(pub &'a ResolvedFrame);

impl crate::expr::Scope for FrameScope<'_> {
    fn var(&self, _name: &str) -> Option<f64> {
        None
    }

    fn node_prop(&self, id: &str, prop: &str) -> Option<f64> {
        self.0.node_prop(id, prop)
    }
}

#[cfg(test)]
mod scan_tests {
    use super::*;

    #[test]
    fn finds_a_single_reference() {
        let refs = scan_node_refs(r#"= node("badge_3", "tx") + 1"#);
        assert_eq!(
            refs,
            vec![NodeRef {
                id: "badge_3".into(),
                prop: "tx".into()
            }]
        );
    }

    #[test]
    fn finds_multiple_references() {
        let refs = scan_node_refs(r#"= node("a", "cx") - node("b", "cy")"#);
        assert_eq!(
            refs,
            vec![
                NodeRef {
                    id: "a".into(),
                    prop: "cx".into()
                },
                NodeRef {
                    id: "b".into(),
                    prop: "cy".into()
                },
            ]
        );
    }

    #[test]
    fn tolerates_whitespace_variations() {
        let refs = scan_node_refs(r#"=node(  "a" ,"tx"  )"#);
        assert_eq!(
            refs,
            vec![NodeRef {
                id: "a".into(),
                prop: "tx".into()
            }]
        );
    }

    #[test]
    fn handles_escaped_quotes_in_the_id() {
        let refs = scan_node_refs(r#"= node("a\"b", "x")"#);
        assert_eq!(
            refs,
            vec![NodeRef {
                id: "a\"b".into(),
                prop: "x".into()
            }]
        );
    }

    #[test]
    fn does_not_match_node_as_a_substring_of_a_longer_identifier() {
        assert!(scan_node_refs(r#"= anode("a", "x") + nodeFoo("b", "y")"#).is_empty());
    }

    #[test]
    fn no_reference_in_plain_arithmetic() {
        assert!(scan_node_refs("= $W / 2 + cos($i)").is_empty());
    }
}

#[cfg(test)]
mod graph_tests {
    use super::*;

    fn nodes(pairs: &[(&str, &[(&str, &str)])]) -> Vec<(String, Vec<NodeRef>)> {
        pairs
            .iter()
            .map(|(id, refs)| {
                (
                    id.to_string(),
                    refs.iter()
                        .map(|(rid, prop)| NodeRef {
                            id: rid.to_string(),
                            prop: prop.to_string(),
                        })
                        .collect(),
                )
            })
            .collect()
    }

    #[test]
    fn independent_nodes_keep_declaration_order() {
        let n = nodes(&[("a", &[]), ("b", &[]), ("c", &[])]);
        let g = DepGraph::build(&n, &HashSet::new()).unwrap();
        assert_eq!(g.order(), &["a", "b", "c"]);
    }

    #[test]
    fn a_dependent_resolves_after_its_dependency_even_when_declared_first() {
        let n = nodes(&[("line", &[("badge", "tx")]), ("badge", &[])]);
        let g = DepGraph::build(&n, &HashSet::new()).unwrap();
        let pos = |id: &str| g.order().iter().position(|x| x == id).unwrap();
        assert!(pos("badge") < pos("line"));
    }

    #[test]
    fn chain_of_three_resolves_in_dependency_order() {
        let n = nodes(&[("c", &[("b", "x")]), ("b", &[("a", "x")]), ("a", &[])]);
        let g = DepGraph::build(&n, &HashSet::new()).unwrap();
        assert_eq!(g.order(), &["a", "b", "c"]);
    }

    #[test]
    fn direct_two_node_cycle_is_reported_with_both_names() {
        let n = nodes(&[("a", &[("b", "x")]), ("b", &[("a", "x")])]);
        let err = DepGraph::build(&n, &HashSet::new()).unwrap_err();
        match err {
            DepsError::Cycle { chain } => {
                assert!(chain.contains(&"a".to_string()));
                assert!(chain.contains(&"b".to_string()));
                assert_eq!(chain.first(), chain.last());
            }
            other => panic!("expected Cycle, got {other:?}"),
        }
    }

    #[test]
    fn three_node_cycle_is_reported() {
        let n = nodes(&[
            ("a", &[("b", "x")]),
            ("b", &[("c", "x")]),
            ("c", &[("a", "x")]),
        ]);
        let err = DepGraph::build(&n, &HashSet::new()).unwrap_err();
        match err {
            DepsError::Cycle { chain } => {
                for id in ["a", "b", "c"] {
                    assert!(
                        chain.contains(&id.to_string()),
                        "chain missing {id}: {chain:?}"
                    );
                }
            }
            other => panic!("expected Cycle, got {other:?}"),
        }
    }

    #[test]
    fn self_reference_is_a_one_node_cycle() {
        let n = nodes(&[("a", &[("a", "x")])]);
        let err = DepGraph::build(&n, &HashSet::new()).unwrap_err();
        assert!(matches!(err, DepsError::Cycle { .. }));
    }

    #[test]
    fn duplicate_id_in_the_same_scene_is_rejected() {
        let n = nodes(&[("dup", &[]), ("other", &[]), ("dup", &[])]);
        let err = DepGraph::build(&n, &HashSet::new()).unwrap_err();
        match err {
            DepsError::DuplicateId(id) => assert_eq!(id, "dup"),
            other => panic!("expected DuplicateId, got {other:?}"),
        }
    }

    #[test]
    fn reference_to_an_id_declared_nowhere_is_unknown() {
        let n = nodes(&[("a", &[("ghost", "x")])]);
        let err = DepGraph::build(&n, &HashSet::new()).unwrap_err();
        match err {
            DepsError::UnknownId {
                referencing,
                target,
            } => {
                assert_eq!(referencing, "a");
                assert_eq!(target, "ghost");
            }
            other => panic!("expected UnknownId, got {other:?}"),
        }
    }

    #[test]
    fn reference_to_an_id_from_another_scene_is_cross_scene() {
        let n = nodes(&[("a", &[("other_scene_node", "x")])]);
        let mut other = HashSet::new();
        other.insert("other_scene_node".to_string());
        let err = DepGraph::build(&n, &other).unwrap_err();
        match err {
            DepsError::CrossScene {
                referencing,
                target,
            } => {
                assert_eq!(referencing, "a");
                assert_eq!(target, "other_scene_node");
            }
            other => panic!("expected CrossScene, got {other:?}"),
        }
    }
}

#[cfg(test)]
mod resolution_tests {
    use super::*;
    use crate::expr::Expr;

    fn layout(x: f32, y: f32, w: f32, h: f32) -> BoxLayout {
        BoxLayout {
            x,
            y,
            width: w,
            height: h,
            ..Default::default()
        }
    }

    #[test]
    fn geometry_family_reads_from_layout() {
        let mut frame = ResolvedFrame::new();
        frame.insert(
            "box",
            ResolvedNode::new(
                layout(10.0, 20.0, 100.0, 50.0),
                (0.0, 0.0, 1.0, 0.0, 1.0),
                None,
            ),
        );
        assert_eq!(frame.node_prop("box", "x"), Some(10.0));
        assert_eq!(frame.node_prop("box", "y"), Some(20.0));
        assert_eq!(frame.node_prop("box", "width"), Some(100.0));
        assert_eq!(frame.node_prop("box", "height"), Some(50.0));
        assert_eq!(frame.node_prop("box", "cx"), Some(60.0));
        assert_eq!(frame.node_prop("box", "cy"), Some(45.0));
        assert_eq!(frame.node_prop("box", "right"), Some(110.0));
        assert_eq!(frame.node_prop("box", "bottom"), Some(70.0));
    }

    #[test]
    fn animated_transform_family_reads_through() {
        let mut frame = ResolvedFrame::new();
        frame.insert(
            "chip",
            ResolvedNode::new(
                layout(0.0, 0.0, 10.0, 10.0),
                (12.5, -3.0, 1.5, 45.0, 0.8),
                None,
            ),
        );
        assert_eq!(frame.node_prop("chip", "tx"), Some(12.5));
        assert_eq!(frame.node_prop("chip", "ty"), Some(-3.0));
        assert_eq!(frame.node_prop("chip", "scale"), Some(1.5));
        assert_eq!(frame.node_prop("chip", "rotation"), Some(45.0));
        assert_eq!(frame.node_prop("chip", "opacity"), Some(0.8_f32 as f64));
    }

    #[test]
    fn text_and_glyph_families_are_none_without_text_metrics() {
        let mut frame = ResolvedFrame::new();
        frame.insert(
            "box",
            ResolvedNode::new(
                layout(0.0, 0.0, 10.0, 10.0),
                (0.0, 0.0, 1.0, 0.0, 1.0),
                None,
            ),
        );
        assert_eq!(frame.node_prop("box", "textWidth"), None);
        assert_eq!(frame.node_prop("box", "glyph_count"), None);
        assert_eq!(frame.node_prop("box", "glyph_x:0"), None);
    }

    #[test]
    fn text_and_glyph_families_read_through() {
        let text = TextMetrics {
            text_width: 88.0,
            cap_height: 14.0,
            ascender: 18.0,
            baseline: 22.0,
            glyphs: vec![
                GlyphMetric {
                    x: 0.0,
                    width: 10.0,
                },
                GlyphMetric {
                    x: 10.0,
                    width: 12.0,
                },
            ],
        };
        let mut frame = ResolvedFrame::new();
        frame.insert(
            "sentence",
            ResolvedNode::new(
                layout(0.0, 0.0, 100.0, 30.0),
                (0.0, 0.0, 1.0, 0.0, 1.0),
                Some(text),
            ),
        );
        assert_eq!(frame.node_prop("sentence", "textWidth"), Some(88.0));
        assert_eq!(frame.node_prop("sentence", "capHeight"), Some(14.0));
        assert_eq!(frame.node_prop("sentence", "ascender"), Some(18.0));
        assert_eq!(frame.node_prop("sentence", "baseline"), Some(22.0));
        assert_eq!(frame.node_prop("sentence", "glyph_count"), Some(2.0));
        assert_eq!(frame.node_prop("sentence", "glyph_x:1"), Some(10.0));
        assert_eq!(frame.node_prop("sentence", "glyph_cx:1"), Some(16.0));
        assert_eq!(frame.node_prop("sentence", "glyph_x:5"), None);
    }

    #[test]
    fn unresolved_reference_surfaces_as_unknown_ident_through_expr() {
        let frame = ResolvedFrame::new();
        let scope = FrameScope(&frame);
        let expr = Expr::parse(r#"= node("missing", "x")"#).unwrap();
        let err = expr.eval(&scope).unwrap_err();
        assert!(matches!(err, crate::expr::ExprError::UnknownIdent(_)));
    }

    #[test]
    fn expression_over_a_resolved_node_matches_hand_computed_value() {
        let mut frame = ResolvedFrame::new();
        frame.insert(
            "chip3",
            ResolvedNode::new(
                layout(500.0, 300.0, 40.0, 40.0),
                (77.0, -12.0, 1.0, 0.0, 1.0),
                None,
            ),
        );
        let scope = FrameScope(&frame);
        let expr = Expr::parse(r#"= node("chip3", "cx") + node("chip3", "tx")"#).unwrap();
        let got = expr.eval(&scope).unwrap();
        assert_eq!(got, 520.0 + 77.0);
    }

    #[test]
    fn orbiting_node_reference_tracks_every_frame_with_no_one_frame_lag() {
        let n = vec![
            (
                "line".to_string(),
                vec![
                    NodeRef {
                        id: "badge".to_string(),
                        prop: "tx".to_string(),
                    },
                    NodeRef {
                        id: "badge".to_string(),
                        prop: "ty".to_string(),
                    },
                ],
            ),
            ("badge".to_string(), vec![]),
        ];
        let graph = DepGraph::build(&n, &HashSet::new()).unwrap();
        assert_eq!(graph.order(), &["badge", "line"]);

        let orbit_transform = |angle_deg: f32| -> (f32, f32, f32, f32, f32) {
            let (s, c) = angle_deg.to_radians().sin_cos();
            (c * 200.0, s * 200.0, 1.0, 0.0, 1.0)
        };

        for angle in [0.0f32, 37.0, 90.0, 181.0, 269.5, 350.0] {
            let mut frame = ResolvedFrame::new();
            for id in graph.order() {
                match id.as_str() {
                    "badge" => {
                        frame.insert(
                            "badge",
                            ResolvedNode::new(
                                layout(0.0, 0.0, 20.0, 20.0),
                                orbit_transform(angle),
                                None,
                            ),
                        );
                    }
                    "line" => {
                        let scope = FrameScope(&frame);
                        let x2 = Expr::parse(r#"= node("badge", "tx")"#)
                            .unwrap()
                            .eval(&scope)
                            .unwrap();
                        let y2 = Expr::parse(r#"= node("badge", "ty")"#)
                            .unwrap()
                            .eval(&scope)
                            .unwrap();
                        frame.insert(
                            "line",
                            ResolvedNode::new(
                                layout(x2 as f32, y2 as f32, 0.0, 0.0),
                                (0.0, 0.0, 1.0, 0.0, 1.0),
                                None,
                            ),
                        );
                    }
                    other => panic!("unexpected id {other}"),
                }
            }

            let (expected_tx, expected_ty, ..) = orbit_transform(angle);
            let line = &frame.nodes["line"];
            assert_eq!(
                line.layout.x, expected_tx,
                "angle {angle}: line.x2 lagged behind badge.tx"
            );
            assert_eq!(
                line.layout.y, expected_ty,
                "angle {angle}: line.y2 lagged behind badge.ty"
            );
        }
    }

    #[test]
    fn resolving_in_declaration_order_instead_of_topological_order_fails_loudly() {
        let mut frame = ResolvedFrame::new();
        let scope = FrameScope(&frame);
        let err = Expr::parse(r#"= node("badge", "tx")"#)
            .unwrap()
            .eval(&scope)
            .unwrap_err();
        assert!(matches!(err, crate::expr::ExprError::UnknownIdent(_)));

        frame.insert(
            "badge",
            ResolvedNode::new(
                layout(0.0, 0.0, 20.0, 20.0),
                (99.0, 0.0, 1.0, 0.0, 1.0),
                None,
            ),
        );
        let scope = FrameScope(&frame);
        let got = Expr::parse(r#"= node("badge", "tx")"#)
            .unwrap()
            .eval(&scope)
            .unwrap();
        assert_eq!(got, 99.0);
    }
}
