//! Box tree — the intermediate representation between a JSON scenario and
//! the layout/paint passes. Each node carries a resolved [`CssStyle`], a
//! discriminator pointing to the source component, and an optional intrinsic
//! measurement callback (text, image, codeblock, chart, ...).

use std::sync::Arc;

use crate::css::CssStyle;

/// Stable identifier for a node within a single layout pass.
pub type NodeId = u32;

/// A renderable box. `kind` is opaque to the engine; the painter dispatch
/// downcasts the inner `Arc<dyn Any>` to the concrete component type when
/// invoked.
pub struct BoxNode {
    pub id: NodeId,
    pub kind: BoxKind,
    pub css: CssStyle,
    pub children: Vec<BoxNode>,
    /// Optional intrinsic measurement (used by taffy's `measure_fn` for
    /// leaves like text / codeblock / image). `None` = pure container.
    pub intrinsic: Option<Arc<dyn IntrinsicMeasure>>,
    /// JSON path of this node relative to its scene's `children` array, e.g.
    /// "/children/2/children/0". `None` for synthetic nodes (the scene root).
    pub source_path: Option<String>,
    /// Visibility window from the component's `start_at`/`end_at` (seconds,
    /// scene-relative). Outside the window the node and its subtree are not
    /// painted but still occupy layout space (CSS `visibility` semantics —
    /// siblings must not jump when the component appears).
    pub window: Option<PaintWindow>,
}

/// Half-open visibility window `[start, end)`; `None` bounds are unbounded.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaintWindow {
    pub start: Option<f64>,
    pub end: Option<f64>,
}

impl PaintWindow {
    pub fn contains(&self, t: f64) -> bool {
        self.start.is_none_or(|s| t >= s) && self.end.is_none_or(|e| t < e)
    }
}

impl BoxNode {
    pub fn container(css: CssStyle, children: Vec<BoxNode>) -> Self {
        Self {
            id: 0,
            kind: BoxKind::Container,
            css,
            children,
            intrinsic: None,
            source_path: None,
            window: None,
        }
    }

    pub fn leaf(css: CssStyle, intrinsic: Arc<dyn IntrinsicMeasure>) -> Self {
        Self {
            id: 0,
            kind: BoxKind::Container,
            css,
            children: Vec::new(),
            intrinsic: Some(intrinsic),
            source_path: None,
            window: None,
        }
    }

    /// Walk the tree and assign sequential `id` values to each node.
    /// Returns the next free id.
    pub fn assign_ids(&mut self, mut next: NodeId) -> NodeId {
        self.id = next;
        next += 1;
        for c in self.children.iter_mut() {
            next = c.assign_ids(next);
        }
        next
    }

    /// Find a node by id.
    pub fn find(&self, id: NodeId) -> Option<&BoxNode> {
        if self.id == id {
            return Some(self);
        }
        for c in &self.children {
            if let Some(r) = c.find(id) {
                return Some(r);
            }
        }
        None
    }
}

/// Discriminates the source component without coupling the engine to its
/// concrete types.
#[derive(Clone)]
pub enum BoxKind {
    /// Generic container — no custom paint, only box decorations.
    Container,
    /// Component-backed leaf or container. Holds an opaque payload that the
    /// dispatcher knows how to handle.
    Component(Arc<dyn std::any::Any + Send + Sync>),
    /// Temporal ghost for motion-blur / trail effects. Painted exactly like
    /// `Component` (same payload, same dispatcher dispatch) but excluded from
    /// the hit-map so the studio never selects a ghost node.
    Ghost(Arc<dyn std::any::Any + Send + Sync>),
}

/// Trait implemented by leaves whose intrinsic size depends on their content.
/// Called by taffy during layout.
pub trait IntrinsicMeasure: Send + Sync {
    /// Measure intrinsic size given the available width/height.
    /// Either side may be `None` if unconstrained (e.g. min-content pass).
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32);
}

/// CSS available-space hint, mirrored from taffy.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AvailableSpace {
    Definite(f32),
    MinContent,
    MaxContent,
}

impl From<taffy::AvailableSpace> for AvailableSpace {
    fn from(v: taffy::AvailableSpace) -> Self {
        match v {
            taffy::AvailableSpace::Definite(p) => Self::Definite(p),
            taffy::AvailableSpace::MinContent => Self::MinContent,
            taffy::AvailableSpace::MaxContent => Self::MaxContent,
        }
    }
}
