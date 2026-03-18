use crate::backend::Backend;
use crate::output;

pub async fn run(
    backend: &mut Backend,
    command: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let cmd_id = output::parse_light_command(command).ok_or_else(|| {
        format!(
            "Unknown light command: {} (use off, on, party, caribbean, blue, green, red, white, purple, ...)",
            command
        )
    })?;

    backend.set_light_command(cmd_id).await?;

    if json {
        output::print_json(&serde_json::json!({
            "command": command,
            "command_id": cmd_id,
        }));
    } else {
        println!("Light command: {}", command);
    }

    Ok(())
}
