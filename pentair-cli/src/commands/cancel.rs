use crate::backend::Backend;
use crate::output;

pub async fn run(backend: &mut Backend, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    backend.cancel_delay().await?;

    if json {
        output::print_json(&serde_json::json!({
            "action": "cancel_delay",
            "success": true,
        }));
    } else {
        println!("All delays cancelled.");
    }

    Ok(())
}
