use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// Re-export existing layout types with clean names
pub use crate::schema::CardAlign as Align;
pub use crate::schema::CardDirection as Direction;
pub use crate::schema::CardJustify as Justify;
pub use crate::schema::GridTrack;

use crate::components::ChildComponent;

/// Trait for components that contain children.
pub trait Container {
    fn children(&self) -> &[ChildComponent];
}

/// Configuration for flex layout.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct FlexConfig {
    #[serde(default)]
    pub direction: Direction,
    #[serde(default)]
    pub justify: Justify,
    #[serde(default)]
    pub align: Align,
    #[serde(default)]
    pub gap: f32,
    #[serde(default)]
    pub wrap: bool,
}

impl Default for FlexConfig {
    fn default() -> Self {
        Self {
            direction: Direction::Column,
            justify: Justify::Start,
            align: Align::Start,
            gap: 0.0,
            wrap: false,
        }
    }
}

/// Trait for flex container components.
pub trait FlexContainer: Container {
    fn flex_config(&self) -> &FlexConfig;
}

/// Mutable access to flex config — needed by builder traits.
pub trait FlexContainerMut: FlexContainer {
    fn flex_config_mut(&mut self) -> &mut FlexConfig;
}

/// Tailwind/GPUI-style builder API for flex containers.
///
/// ```ignore
/// let flex = Flex::new()
///     .flex_row()
///     .items_center()
///     .justify_between()
///     .gap_4()
///     .flex_wrap();
/// ```
pub trait FlexContainerExt: FlexContainerMut + Sized {
    // --- Direction ---

    fn flex_row(mut self) -> Self {
        self.flex_config_mut().direction = Direction::Row;
        self
    }

    fn flex_col(mut self) -> Self {
        self.flex_config_mut().direction = Direction::Column;
        self
    }

    fn flex_row_reverse(mut self) -> Self {
        self.flex_config_mut().direction = Direction::RowReverse;
        self
    }

    fn flex_col_reverse(mut self) -> Self {
        self.flex_config_mut().direction = Direction::ColumnReverse;
        self
    }

    // --- Align (cross-axis) ---

    fn items_start(mut self) -> Self {
        self.flex_config_mut().align = Align::Start;
        self
    }

    fn items_center(mut self) -> Self {
        self.flex_config_mut().align = Align::Center;
        self
    }

    fn items_end(mut self) -> Self {
        self.flex_config_mut().align = Align::End;
        self
    }

    fn items_stretch(mut self) -> Self {
        self.flex_config_mut().align = Align::Stretch;
        self
    }

    // --- Justify (main-axis) ---

    fn justify_start(mut self) -> Self {
        self.flex_config_mut().justify = Justify::Start;
        self
    }

    fn justify_center(mut self) -> Self {
        self.flex_config_mut().justify = Justify::Center;
        self
    }

    fn justify_end(mut self) -> Self {
        self.flex_config_mut().justify = Justify::End;
        self
    }

    fn justify_between(mut self) -> Self {
        self.flex_config_mut().justify = Justify::SpaceBetween;
        self
    }

    fn justify_around(mut self) -> Self {
        self.flex_config_mut().justify = Justify::SpaceAround;
        self
    }

    fn justify_evenly(mut self) -> Self {
        self.flex_config_mut().justify = Justify::SpaceEvenly;
        self
    }

    // --- Gap ---

    fn gap(mut self, value: f32) -> Self {
        self.flex_config_mut().gap = value;
        self
    }

    fn gap_0(self) -> Self { self.gap(0.0) }
    fn gap_1(self) -> Self { self.gap(4.0) }
    fn gap_2(self) -> Self { self.gap(8.0) }
    fn gap_3(self) -> Self { self.gap(12.0) }
    fn gap_4(self) -> Self { self.gap(16.0) }
    fn gap_5(self) -> Self { self.gap(20.0) }
    fn gap_6(self) -> Self { self.gap(24.0) }
    fn gap_8(self) -> Self { self.gap(32.0) }

    // --- Wrap ---

    fn flex_wrap(mut self) -> Self {
        self.flex_config_mut().wrap = true;
        self
    }

    fn flex_nowrap(mut self) -> Self {
        self.flex_config_mut().wrap = false;
        self
    }
}

/// Blanket impl: any type implementing `FlexContainerMut` gets all flex builder methods.
impl<T: FlexContainerMut + Sized> FlexContainerExt for T {}

/// Configuration for grid layout.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GridConfig {
    #[serde(default)]
    pub grid_template_columns: Option<Vec<GridTrack>>,
    #[serde(default)]
    pub grid_template_rows: Option<Vec<GridTrack>>,
    #[serde(default)]
    pub gap: f32,
}

impl Default for GridConfig {
    fn default() -> Self {
        Self {
            grid_template_columns: None,
            grid_template_rows: None,
            gap: 0.0,
        }
    }
}

/// Trait for grid container components.
pub trait GridContainer: Container {
    fn grid_config(&self) -> &GridConfig;
}

/// Mutable access to grid config — needed by builder traits.
pub trait GridContainerMut: GridContainer {
    fn grid_config_mut(&mut self) -> &mut GridConfig;
}

/// Builder API for grid containers.
pub trait GridContainerExt: GridContainerMut + Sized {
    fn grid_gap(mut self, value: f32) -> Self {
        self.grid_config_mut().gap = value;
        self
    }

    fn grid_gap_0(self) -> Self { self.grid_gap(0.0) }
    fn grid_gap_1(self) -> Self { self.grid_gap(4.0) }
    fn grid_gap_2(self) -> Self { self.grid_gap(8.0) }
    fn grid_gap_4(self) -> Self { self.grid_gap(16.0) }
    fn grid_gap_8(self) -> Self { self.grid_gap(32.0) }

    fn grid_cols(mut self, cols: Vec<GridTrack>) -> Self {
        self.grid_config_mut().grid_template_columns = Some(cols);
        self
    }

    fn grid_rows(mut self, rows: Vec<GridTrack>) -> Self {
        self.grid_config_mut().grid_template_rows = Some(rows);
        self
    }
}

/// Blanket impl: any type implementing `GridContainerMut` gets all grid builder methods.
impl<T: GridContainerMut + Sized> GridContainerExt for T {}
