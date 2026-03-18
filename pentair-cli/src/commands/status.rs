use crate::backend::Backend;
use crate::output;

pub async fn run(backend: &mut Backend, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let status = backend.get_status().await?;
    let config = backend.get_controller_config().await?;

    if json {
        // Merge status and config into a combined JSON object
        let mut status_val = serde_json::to_value(&status)?;
        let config_val = serde_json::to_value(&config)?;
        if let Some(obj) = status_val.as_object_mut() {
            obj.insert("config".to_string(), config_val);
        }
        output::print_json(&status_val);
        return Ok(());
    }

    // Text output
    let unit = if config.is_celsius { "C" } else { "F" };
    println!("Air Temperature: {}{}", status.air_temp, unit);
    println!();

    for body in &status.bodies {
        let name = output::format_body_type(body.body_type);
        println!("{}:", name);
        println!("  Temperature:   {}{}", body.current_temp, unit);
        println!("  Set Point:     {}{}", body.set_point, unit);
        println!("  Cool Set Point:{}{}", body.cool_set_point, unit);
        println!("  Heat Mode:     {}", output::format_heat_mode(body.heat_mode));
        println!(
            "  Heat Status:   {}",
            output::format_heat_status(body.heat_status)
        );
        println!();
    }

    println!("Circuits:");
    for cs in &status.circuits {
        // Find circuit name from config
        let name = config
            .circuits
            .iter()
            .find(|c| c.circuit_id == cs.circuit_id)
            .map(|c| c.name.as_str())
            .unwrap_or("Unknown");
        let state = if cs.state { "ON" } else { "OFF" };
        println!("  {:20} {}", name, state);
    }

    if status.freeze_mode {
        println!();
        println!("** FREEZE PROTECTION ACTIVE **");
    }

    Ok(())
}
