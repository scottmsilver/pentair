use pentair_protocol::responses::ControllerConfig;

/// Print a value as JSON.
pub fn print_json<T: serde::Serialize>(value: &T) {
    println!("{}", serde_json::to_string_pretty(value).unwrap());
}

/// Resolve a circuit name or ID to a logical circuit ID, using the controller config.
/// Accepts either a numeric ID or a case-insensitive circuit name.
pub fn resolve_circuit(name_or_id: &str, config: &ControllerConfig) -> Option<i32> {
    // Try parsing as number first
    if let Ok(id) = name_or_id.parse::<i32>() {
        return Some(id);
    }

    // Case-insensitive name match
    let lower = name_or_id.to_lowercase();
    for circ in &config.circuits {
        if circ.name.to_lowercase() == lower {
            return Some(circ.circuit_id - 499); // Convert wire ID to logical
        }
    }
    None
}

/// Parse body type string to protocol i32.
pub fn parse_body(s: &str) -> Option<i32> {
    match s.to_lowercase().as_str() {
        "pool" => Some(0),
        "spa" => Some(1),
        _ => None,
    }
}

/// Parse heat mode string to protocol i32.
pub fn parse_heat_mode(s: &str) -> Option<i32> {
    match s.to_lowercase().as_str() {
        "off" => Some(0),
        "solar" => Some(1),
        "solar-preferred" | "solarpref" | "solar-pref" => Some(2),
        "heat-pump" | "heatpump" | "heater" => Some(3),
        _ => None,
    }
}

/// Parse light command string to protocol i32.
pub fn parse_light_command(s: &str) -> Option<i32> {
    match s.to_lowercase().as_str() {
        "off" => Some(0),
        "on" => Some(1),
        "set" => Some(2),
        "sync" => Some(3),
        "swim" => Some(4),
        "party" => Some(5),
        "romantic" => Some(6),
        "caribbean" => Some(7),
        "american" => Some(8),
        "sunset" => Some(9),
        "royal" => Some(10),
        "save" => Some(11),
        "recall" => Some(12),
        "blue" => Some(13),
        "green" => Some(14),
        "red" => Some(15),
        "white" => Some(16),
        "purple" => Some(17),
        _ => None,
    }
}

/// Parse time string "HH:MM" to minutes from midnight.
pub fn parse_time(s: &str) -> Option<i32> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() != 2 {
        return None;
    }
    let hour: i32 = parts[0].parse().ok()?;
    let minute: i32 = parts[1].parse().ok()?;
    Some(hour * 60 + minute)
}

/// Parse day mask string to bitmask.
/// Accepts "MoTuWeThFrSaSu" format, or "daily", "weekdays", "weekends".
pub fn parse_day_mask(s: &str) -> Option<i32> {
    match s.to_lowercase().as_str() {
        "daily" | "everyday" => Some(0x7F),
        "weekdays" => Some(0x1F), // Mon-Fri
        "weekends" => Some(0x60), // Sat-Sun
        _ => {
            let mut mask = 0i32;
            let lower = s.to_lowercase();
            if lower.contains("mo") {
                mask |= 0x01;
            }
            if lower.contains("tu") {
                mask |= 0x02;
            }
            if lower.contains("we") {
                mask |= 0x04;
            }
            if lower.contains("th") {
                mask |= 0x08;
            }
            if lower.contains("fr") {
                mask |= 0x10;
            }
            if lower.contains("sa") {
                mask |= 0x20;
            }
            if lower.contains("su") {
                mask |= 0x40;
            }
            if mask == 0 {
                None
            } else {
                Some(mask)
            }
        }
    }
}

/// Format minutes from midnight as HH:MM.
pub fn format_time(minutes: u32) -> String {
    format!("{:02}:{:02}", minutes / 60, minutes % 60)
}

/// Format day mask as readable string.
pub fn format_days(mask: u32) -> String {
    let days = ["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"];
    let mut result = Vec::new();
    for (i, day) in days.iter().enumerate() {
        if mask & (1 << i) != 0 {
            result.push(*day);
        }
    }
    if result.len() == 7 {
        "Daily".to_string()
    } else if result == ["Mo", "Tu", "We", "Th", "Fr"] {
        "Weekdays".to_string()
    } else if result == ["Sa", "Su"] {
        "Weekends".to_string()
    } else {
        result.join("")
    }
}

/// Format a heat mode integer as a readable string.
pub fn format_heat_mode(mode: i32) -> &'static str {
    match mode {
        0 => "Off",
        1 => "Solar",
        2 => "Solar Preferred",
        3 => "Heat Pump",
        _ => "Unknown",
    }
}

/// Format a heat status integer as a readable string.
pub fn format_heat_status(status: i32) -> &'static str {
    match status {
        0 => "Off",
        1 => "Solar",
        2 => "Heater",
        3 => "Both",
        _ => "Unknown",
    }
}

/// Format a body type integer as a readable string.
pub fn format_body_type(body: i32) -> &'static str {
    match body {
        0 => "Pool",
        1 => "Spa",
        _ => "Unknown",
    }
}

/// Format a circuit function byte as a readable string.
pub fn format_circuit_function(func: u8) -> &'static str {
    match func {
        0 => "Generic",
        1 => "Spa",
        2 => "Pool",
        5 => "Master Cleaner",
        7 => "Light",
        9 => "SAM Light",
        10 => "SAL Light",
        11 => "Photon Gen",
        12 => "Color Wheel",
        13 => "Valve",
        14 => "Spillway",
        16 => "IntelliBrite",
        17 => "Floor",
        19 => "Color Logic",
        _ => "Unknown",
    }
}

/// Format a pump type integer as a readable string.
pub fn format_pump_type(pump_type: u32) -> &'static str {
    match pump_type {
        0 => "None",
        1 => "VF (Variable Flow)",
        2 => "VS (Variable Speed)",
        3 => "VSF (Variable Speed/Flow)",
        _ => "Unknown",
    }
}
