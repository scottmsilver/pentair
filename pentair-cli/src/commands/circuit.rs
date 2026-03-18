use crate::backend::Backend;
use crate::output;

pub async fn list(backend: &mut Backend, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let config = backend.get_controller_config().await?;

    if json {
        output::print_json(&config.circuits);
        return Ok(());
    }

    println!("{:<6} {:20} Function", "ID", "Name");
    println!("{}", "-".repeat(50));
    for circ in &config.circuits {
        let logical_id = circ.circuit_id - 499;
        println!(
            "{:<6} {:20} {}",
            logical_id,
            circ.name,
            output::format_circuit_function(circ.function)
        );
    }

    Ok(())
}

pub async fn set(
    backend: &mut Backend,
    circuit: &str,
    on: bool,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = backend.get_controller_config().await?;
    let circuit_id = output::resolve_circuit(circuit, &config)
        .ok_or_else(|| format!("Unknown circuit: {}", circuit))?;

    backend.set_circuit(circuit_id, on).await?;

    let state_str = if on { "ON" } else { "OFF" };
    if json {
        output::print_json(&serde_json::json!({
            "circuit": circuit,
            "circuit_id": circuit_id,
            "state": state_str,
        }));
    } else {
        println!("Circuit {} set to {}", circuit, state_str);
    }

    Ok(())
}
