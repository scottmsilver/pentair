use crate::backend::Backend;
use crate::output;

pub async fn list(
    backend: &mut Backend,
    schedule_type: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let stype = parse_schedule_type(schedule_type)?;
    let data = backend.get_schedule_data(stype).await?;

    if json {
        output::print_json(&data);
        return Ok(());
    }

    if data.events.is_empty() {
        println!("No {} schedules.", schedule_type);
        return Ok(());
    }

    println!(
        "{:<4} {:<10} {:<8} {:<8} {:<12} {:<6}",
        "ID", "Circuit", "Start", "Stop", "Days", "Heat"
    );
    println!("{}", "-".repeat(54));
    for event in &data.events {
        println!(
            "{:<4} {:<10} {:<8} {:<8} {:<12} {}",
            event.schedule_id,
            event.circuit_id,
            output::format_time(event.start_time),
            output::format_time(event.stop_time),
            output::format_days(event.day_mask),
            if event.heat_set_point > 0 {
                format!("{}F", event.heat_set_point)
            } else {
                "-".to_string()
            },
        );
    }

    Ok(())
}

pub async fn add(
    backend: &mut Backend,
    schedule_type: &str,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let stype = parse_schedule_type(schedule_type)?;
    backend.add_schedule_event(stype).await?;

    if json {
        output::print_json(&serde_json::json!({
            "action": "add",
            "schedule_type": schedule_type,
        }));
    } else {
        println!("Added new {} schedule event.", schedule_type);
    }

    Ok(())
}

pub async fn delete(
    backend: &mut Backend,
    id: i32,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    backend.delete_schedule_event(id).await?;

    if json {
        output::print_json(&serde_json::json!({
            "action": "delete",
            "schedule_id": id,
        }));
    } else {
        println!("Deleted schedule event {}.", id);
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub async fn set(
    backend: &mut Backend,
    id: i32,
    circuit: &str,
    start: &str,
    stop: &str,
    days: &str,
    heat: i32,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = backend.get_controller_config().await?;
    let circuit_id = output::resolve_circuit(circuit, &config)
        .ok_or_else(|| format!("Unknown circuit: {}", circuit))?;

    let start_time = output::parse_time(start)
        .ok_or_else(|| format!("Invalid start time: {} (use HH:MM)", start))?;
    let stop_time = output::parse_time(stop)
        .ok_or_else(|| format!("Invalid stop time: {} (use HH:MM)", stop))?;
    let day_mask = output::parse_day_mask(days).ok_or_else(|| {
        format!(
            "Invalid days: {} (use MoTuWe... or daily/weekdays/weekends)",
            days
        )
    })?;

    backend
        .set_schedule_event(id, circuit_id, start_time, stop_time, day_mask, heat)
        .await?;

    if json {
        output::print_json(&serde_json::json!({
            "schedule_id": id,
            "circuit": circuit,
            "circuit_id": circuit_id,
            "start": start,
            "stop": stop,
            "days": days,
            "heat_set_point": heat,
        }));
    } else {
        println!("Updated schedule event {}.", id);
    }

    Ok(())
}

fn parse_schedule_type(s: &str) -> Result<i32, Box<dyn std::error::Error>> {
    match s.to_lowercase().as_str() {
        "recurring" | "0" => Ok(0),
        "runonce" | "run-once" | "once" | "1" => Ok(1),
        _ => Err(format!("Unknown schedule type: {} (use recurring or runonce)", s).into()),
    }
}
