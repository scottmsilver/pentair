use chrono::{Duration, NaiveDate, NaiveDateTime};

use crate::backend::Backend;
use crate::output;

use pentair_protocol::responses::{TimeRangePoint, TimeTempPoint};
use pentair_protocol::types::SLDateTime;

pub async fn run(
    backend: &mut Backend,
    hours: i64,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if hours <= 0 {
        return Err("hours must be greater than zero".into());
    }

    let system_time = backend.get_system_time().await?;
    let end = sl_to_naive(&system_time.time)?;
    let start = end - Duration::hours(hours);
    let sender_id = backend.client_id()?;
    let start_sl = naive_to_sl(&start)?;
    let end_sl = naive_to_sl(&end)?;
    let history = backend.get_history(&start_sl, &end_sl, sender_id).await?;

    if json {
        output::print_json(&history);
        return Ok(());
    }

    println!(
        "History window: {} to {}",
        format_naive(&start),
        format_naive(&end)
    );
    println!();

    print_latest_temp("Air", &history.air_temps);
    print_latest_temp("Pool", &history.pool_temps);
    print_latest_temp("Pool set point", &history.pool_set_point_temps);
    print_latest_temp("Spa", &history.spa_temps);
    print_latest_temp("Spa set point", &history.spa_set_point_temps);
    println!();

    print_last_run("Pool", &history.pool_runs);
    print_last_run("Spa", &history.spa_runs);
    print_last_run("Solar", &history.solar_runs);
    print_last_run("Heater", &history.heater_runs);
    print_last_run("Lights", &history.light_runs);

    Ok(())
}

fn sl_to_naive(dt: &SLDateTime) -> Result<NaiveDateTime, Box<dyn std::error::Error>> {
    let date = NaiveDate::from_ymd_opt(dt.year as i32, dt.month as u32, dt.day as u32)
        .ok_or_else(|| format!("invalid controller date: {dt:?}"))?;
    date.and_hms_milli_opt(
        dt.hour as u32,
        dt.minute as u32,
        dt.second as u32,
        dt.millisecond as u32,
    )
    .ok_or_else(|| format!("invalid controller time: {dt:?}").into())
}

fn naive_to_sl(dt: &NaiveDateTime) -> Result<SLDateTime, Box<dyn std::error::Error>> {
    Ok(SLDateTime {
        year: dt.year().try_into()?,
        month: dt.month().try_into()?,
        day_of_week: dt.weekday().num_days_from_sunday().try_into()?,
        day: dt.day().try_into()?,
        hour: dt.hour().try_into()?,
        minute: dt.minute().try_into()?,
        second: dt.second().try_into()?,
        millisecond: dt.and_utc().timestamp_subsec_millis().try_into()?,
    })
}

fn format_naive(dt: &NaiveDateTime) -> String {
    dt.format("%Y-%m-%d %H:%M:%S").to_string()
}

fn format_sl(dt: &SLDateTime) -> String {
    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02}",
        dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
    )
}

fn print_latest_temp(label: &str, points: &[TimeTempPoint]) {
    match points.last() {
        Some(point) => println!(
            "{label:14} {} at {} ({} samples)",
            point.temp,
            format_sl(&point.time),
            points.len()
        ),
        None => println!("{label:14} no samples"),
    }
}

fn print_last_run(label: &str, runs: &[TimeRangePoint]) {
    match runs.last() {
        Some(run) => println!(
            "{label:14} {} to {} ({} runs)",
            format_sl(&run.on),
            format_sl(&run.off),
            runs.len()
        ),
        None => println!("{label:14} no runs"),
    }
}

use chrono::Datelike;
use chrono::Timelike;
