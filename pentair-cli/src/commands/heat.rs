use crate::backend::Backend;
use crate::output;

pub async fn status(backend: &mut Backend, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let status = backend.get_status().await?;
    let config = backend.get_controller_config().await?;

    if json {
        output::print_json(&status.bodies);
        return Ok(());
    }

    let unit = if config.is_celsius { "C" } else { "F" };
    for body in &status.bodies {
        let name = output::format_body_type(body.body_type);
        println!("{}:", name);
        println!("  Temperature:    {}{}", body.current_temp, unit);
        println!("  Set Point:      {}{}", body.set_point, unit);
        println!("  Cool Set Point: {}{}", body.cool_set_point, unit);
        println!(
            "  Heat Mode:      {}",
            output::format_heat_mode(body.heat_mode)
        );
        println!(
            "  Heat Status:    {}",
            output::format_heat_status(body.heat_status)
        );
        println!();
    }

    Ok(())
}

pub async fn set(
    backend: &mut Backend,
    body: &str,
    temp: i32,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let body_type = output::parse_body(body)
        .ok_or_else(|| format!("Unknown body: {} (use pool or spa)", body))?;

    backend.set_heat_setpoint(body_type, temp).await?;

    if json {
        output::print_json(&serde_json::json!({
            "body": body,
            "set_point": temp,
        }));
    } else {
        println!("Set {} heat set point to {}", body, temp);
    }

    Ok(())
}

pub async fn mode(
    backend: &mut Backend,
    body: &str,
    mode: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let body_type = output::parse_body(body)
        .ok_or_else(|| format!("Unknown body: {} (use pool or spa)", body))?;
    let heat_mode = output::parse_heat_mode(mode).ok_or_else(|| {
        format!(
            "Unknown heat mode: {} (use off, solar, solar-preferred, heat-pump)",
            mode
        )
    })?;

    backend.set_heat_mode(body_type, heat_mode).await?;

    if json {
        output::print_json(&serde_json::json!({
            "body": body,
            "mode": mode,
        }));
    } else {
        println!("Set {} heat mode to {}", body, mode);
    }

    Ok(())
}

pub async fn cool(
    backend: &mut Backend,
    body: &str,
    temp: i32,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let body_type = output::parse_body(body)
        .ok_or_else(|| format!("Unknown body: {} (use pool or spa)", body))?;

    backend.set_cool_setpoint(body_type, temp).await?;

    if json {
        output::print_json(&serde_json::json!({
            "body": body,
            "cool_set_point": temp,
        }));
    } else {
        println!("Set {} cool set point to {}", body, temp);
    }

    Ok(())
}
