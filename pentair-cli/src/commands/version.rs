use crate::backend::Backend;
use crate::output;

pub async fn run(backend: &mut Backend, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let version = backend.get_version().await?;

    if json {
        output::print_json(&version);
    } else {
        println!("{}", version.version);
    }

    Ok(())
}
