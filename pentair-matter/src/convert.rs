/// Convert Fahrenheit (integer) to Matter temperature (0.01°C units, i16).
pub fn fahrenheit_to_matter(f: i32) -> i16 {
    let celsius = (f as f64 - 32.0) * 5.0 / 9.0;
    (celsius * 100.0).round() as i16
}

/// Convert Matter temperature (0.01°C units, i16) to Fahrenheit (rounded integer).
pub fn matter_to_fahrenheit(matter: i16) -> i32 {
    let celsius = matter as f64 / 100.0;
    (celsius * 9.0 / 5.0 + 32.0).round() as i32
}

/// Map Pentair heat_mode string to Matter SystemMode.
/// Matter SystemMode: 0=Off, 1=Auto, 3=Cool, 4=Heat
pub fn pentair_heat_mode_to_matter(mode: &str) -> u8 {
    match mode {
        "off" => 0,
        _ => 4,  // Heat (solar, solar-preferred, heat-pump all map to "Heat")
    }
}

/// Map Matter SystemMode to Pentair heat_mode action.
/// Returns None for unsupported modes (Cool, Auto).
#[allow(dead_code)]
pub fn matter_to_pentair_heat_mode(system_mode: u8) -> Option<&'static str> {
    match system_mode {
        0 => Some("off"),
        4 => Some("heat"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn freezing_point() {
        assert_eq!(fahrenheit_to_matter(32), 0);
        assert_eq!(matter_to_fahrenheit(0), 32);
    }

    #[test]
    fn spa_temperature_104f() {
        let matter = fahrenheit_to_matter(104);
        assert_eq!(matter, 4000);
        assert_eq!(matter_to_fahrenheit(4000), 104);
    }

    #[test]
    fn zero_fahrenheit() {
        let matter = fahrenheit_to_matter(0);
        assert_eq!(matter, -1778);
        assert_eq!(matter_to_fahrenheit(-1778), 0);
    }

    #[test]
    fn boiling_point() {
        assert_eq!(fahrenheit_to_matter(212), 10000);
        assert_eq!(matter_to_fahrenheit(10000), 212);
    }

    #[test]
    fn round_trip_common_spa_temps() {
        for f in [98, 100, 102, 104, 106] {
            let matter = fahrenheit_to_matter(f);
            let back = matter_to_fahrenheit(matter);
            assert_eq!(back, f, "Round trip failed for {}°F", f);
        }
    }

    #[test]
    fn heat_mode_mapping() {
        assert_eq!(pentair_heat_mode_to_matter("off"), 0);
        assert_eq!(pentair_heat_mode_to_matter("heat-pump"), 4);
        assert_eq!(pentair_heat_mode_to_matter("solar"), 4);
        assert_eq!(pentair_heat_mode_to_matter("solar-preferred"), 4);
    }

    #[test]
    fn matter_mode_mapping() {
        assert_eq!(matter_to_pentair_heat_mode(0), Some("off"));
        assert_eq!(matter_to_pentair_heat_mode(4), Some("heat"));
        assert_eq!(matter_to_pentair_heat_mode(1), None);
        assert_eq!(matter_to_pentair_heat_mode(3), None);
    }
}
