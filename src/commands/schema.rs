use crate::error::Result;
use crate::schema;

pub fn cmd_schema(output: Option<&std::path::Path>) -> Result<()> {
    let schema = schema::generate_json_schema();
    let json = serde_json::to_string_pretty(&schema)?;

    if let Some(path) = output {
        std::fs::write(path, &json)?;
        eprintln!("Schema written to {}", path.display());
    } else {
        println!("{}", json);
    }

    Ok(())
}
