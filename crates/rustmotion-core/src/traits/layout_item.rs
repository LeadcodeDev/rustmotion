use super::{Styled, Widget};

/// Abstraction for items that participate in layout (flex, grid, stack).
/// Breaks the dependency from layout/ → components/ by providing a trait
/// that ChildComponent implements, so layout code operates on `&[impl LayoutItem]`
/// instead of `&[ChildComponent]` directly.
pub trait LayoutItem {
    fn as_widget(&self) -> &dyn Widget;
    fn as_styled(&self) -> &dyn Styled;
    fn is_container(&self) -> bool;
    fn is_flow(&self) -> bool;
    fn is_decorative(&self) -> bool;
    fn absolute_position(&self) -> Option<(f32, f32)>;

    /// Fallback position for stack layout when `absolute_position()` returns `None`.
    /// Defaults to `(0.0, 0.0)`.
    fn default_position(&self) -> (f32, f32) {
        (0.0, 0.0)
    }

    /// Per-child flex properties (delegated to the child's style).
    fn flex_basis(&self) -> Option<f32> {
        self.as_styled().style_config().flex_basis
    }
    fn flex_grow(&self) -> Option<f32> {
        self.as_styled().style_config().flex_grow
    }
    fn flex_shrink(&self) -> Option<f32> {
        self.as_styled().style_config().flex_shrink
    }
    fn align_self(&self) -> Option<super::Align> {
        self.as_styled().style_config().align_self.clone()
    }
    fn grid_column(&self) -> Option<&crate::schema::GridPlacement> {
        self.as_styled().style_config().grid_column.as_ref()
    }
    fn grid_row(&self) -> Option<&crate::schema::GridPlacement> {
        self.as_styled().style_config().grid_row.as_ref()
    }
}
