/// Result of the layout pass — a tree of resolved positions and sizes.
#[derive(Debug, Clone)]
pub struct LayoutNode {
    /// Position relative to parent's content area.
    pub x: f32,
    pub y: f32,
    /// Resolved size of this node.
    pub width: f32,
    pub height: f32,
    /// Layout results for children.
    pub children: Vec<LayoutNode>,
}

impl LayoutNode {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self {
            x,
            y,
            width,
            height,
            children: Vec::new(),
        }
    }

    pub fn with_children(mut self, children: Vec<LayoutNode>) -> Self {
        self.children = children;
        self
    }
}

impl Default for LayoutNode {
    fn default() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 0.0,
            height: 0.0,
            children: Vec::new(),
        }
    }
}
