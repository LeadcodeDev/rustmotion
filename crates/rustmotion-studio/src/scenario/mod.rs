mod baseline;
mod diff;
mod edit;
mod history;
mod model;
mod optimistic;
mod sidecar;

pub use baseline::{baseline_slot, get_baseline, set_baseline};
pub use diff::{diff_scenarios, ChangeKind, ElementChange};
pub use edit::{
    append_annotation, list_annotations, read_field, read_style_object, remove_annotation,
    scene_duration_for_pointer, set_field, set_field_value, set_style, set_style_value,
};
pub use history::{history_slot, record_edit, redo, set_saving, undo, SharedHistory};
pub use model::{empty_scenario, Shared, StudioModel};
pub use optimistic::{
    apply_optimistic, is_self_write, note_self_write, pending_write_slot, queue_mutation,
    resolve_flush, resolve_for_render, self_write_slot, take_pending, Mutation, PendingWrites,
};
pub use sidecar::{
    append_sidecar_annotation, merge_annotations, read_sidecar, remove_sidecar_annotation,
};

#[derive(Clone, Copy, PartialEq)]
pub enum View {
    Library,
    Editor,
}
