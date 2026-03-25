pub mod animation;
pub mod background;
pub mod codeblock_types;
pub mod scenario;
pub mod style;
pub mod video;

pub use animation::*;
pub use background::*;
pub use codeblock_types::*;
pub use scenario::*;
pub use style::*;
pub use video::*;

pub fn generate_json_schema() -> serde_json::Value {
    let schema = schemars::schema_for!(scenario::Scenario);
    serde_json::to_value(schema).unwrap()
}
