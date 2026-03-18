use crate::backend::Backend;
use crate::output;

pub async fn run(backend: &mut Backend, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let weather = backend.get_weather().await?;

    if json {
        output::print_json(&weather);
        return Ok(());
    }

    println!("Weather:");
    if !weather.description.is_empty() {
        println!("  Conditions:  {}", weather.description);
    }
    println!("  Temperature: {}", weather.current_temp);
    println!("  Humidity:    {}%", weather.humidity);
    if !weather.wind.is_empty() {
        println!("  Wind:        {}", weather.wind);
    }
    println!("  Dew Point:   {}", weather.dew_point);
    println!(
        "  Sunrise:     {}",
        output::format_time(weather.sunrise as u32)
    );
    println!(
        "  Sunset:      {}",
        output::format_time(weather.sunset as u32)
    );

    if !weather.forecast_days.is_empty() {
        println!();
        println!("Forecast:");
        for day in &weather.forecast_days {
            println!(
                "  {}: High {} / Low {} - {}",
                day.text, day.high_temp, day.low_temp, day.text
            );
        }
    }

    Ok(())
}
