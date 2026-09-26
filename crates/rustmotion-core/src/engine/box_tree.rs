use std::sync::Arc;

use crate::css::CssStyle;

pub type NodeId = u32;

pub struct BoxNode {
    pub id: NodeId,
    pub kind: BoxKind,
    pub css: CssStyle,
    pub children: Vec<BoxNode>,
    pub intrinsic: Option<Arc<dyn IntrinsicMeasure>>,
    pub source_path: Option<String>,
    pub window: Option<PaintWindow>,
}

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

    pub fn assign_ids(&mut self, mut next: NodeId) -> NodeId {
        self.id = next;
        next += 1;
        for c in self.children.iter_mut() {
            next = c.assign_ids(next);
        }
        next
    }

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

#[derive(Clone)]
pub enum BoxKind {
    Container,
    Component(Arc<dyn std::any::Any + Send + Sync>),
    Ghost(Arc<dyn std::any::Any + Send + Sync>),
}

pub trait IntrinsicMeasure: Send + Sync {
    fn measure(
        &self,
        known: (Option<f32>, Option<f32>),
        available: (AvailableSpace, AvailableSpace),
    ) -> (f32, f32);
}

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
