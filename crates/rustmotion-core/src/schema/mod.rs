pub mod animation;
pub mod background;
pub mod scenario;
pub mod shake;
pub mod style;
pub mod time;
pub mod video;

pub use animation::*;
pub use background::*;
pub use scenario::*;
pub use shake::*;
pub use style::*;
pub use time::*;
pub use video::*;

pub fn generate_json_schema() -> serde_json::Value {
    let schema = schemars::schema_for!(scenario::Scenario);
    serde_json::to_value(schema).unwrap()
}
