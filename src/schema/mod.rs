pub mod animation;
pub mod video;

pub use animation::*;
pub use video::*;

pub fn generate_json_schema() -> serde_json::Value {
    let schema = schemars::schema_for!(video::Scenario);
    serde_json::to_value(schema).unwrap()
}
