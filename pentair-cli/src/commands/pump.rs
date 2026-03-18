use crate::backend::Backend;
use crate::output;

pub async fn run(
    backend: &mut Backend,
    index: i32,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let pump = backend.get_pump_status(index).await?;

    if json {
        output::print_json(&pump);
        return Ok(());
    }

    if pump.pump_type == 0 {
        println!("Pump {}: not installed", index);
        return Ok(());
    }

    println!("Pump {}:", index);
    println!("  Type:    {}", output::format_pump_type(pump.pump_type));
    println!(
        "  Status:  {}",
        if pump.is_running { "Running" } else { "Stopped" }
    );
    println!("  Watts:   {}", pump.watts);
    println!("  RPM:     {}", pump.rpm);
    println!("  GPM:     {}", pump.gpm);

    let active_circuits: Vec<_> = pump.circuits.iter().filter(|c| c.circuit_id != 0).collect();
    if !active_circuits.is_empty() {
        println!("  Circuits:");
        for circ in active_circuits {
            let speed_unit = if circ.is_rpm { "RPM" } else { "GPM" };
            println!(
                "    Circuit {}: {} {}",
                circ.circuit_id, circ.speed, speed_unit
            );
        }
    }

    Ok(())
}
