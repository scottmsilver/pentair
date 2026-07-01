//! `pentair heat-plan` — print the daemon's ADVISORY pool comfort heat plan.
//!
//! This command is strictly read-only. With no flag it performs a single GET
//! against the daemon's `/api/pool/heat-plan` endpoint and pretty-prints the
//! recommended schedule + projected savings. With `--backtest` it replays the
//! recorded weather log (`~/.config/pool-temp/logs/monitor.jsonl`) as the
//! forecast and runs the optimizer vs a constant-setpoint baseline locally,
//! printing a MODEL-PROJECTED savings report. Either way it NEVER actuates — it
//! sends no setpoint, no heat/on command, nothing. The plan is advice for a
//! human, not a control loop.

use crate::backend::Backend;
use crate::output;

use pentair_daemon::scheduler::{
    self, BacktestInput, ComfortWindow, GasHeaterModel, GasRate, HhMm, RateSchedule, Weekday,
};
use pentair_daemon::thermal::{CoolingParams, WeatherSegment};

pub async fn run(
    backend: &mut Backend,
    backtest: bool,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    if backtest {
        return run_backtest(json);
    }

    let plan = backend.get_heat_plan().await?;

    if json {
        output::print_json(&plan);
        return Ok(());
    }

    // Disabled => feature off (no comfort windows configured).
    if plan.get("enabled").and_then(|v| v.as_bool()) != Some(true) {
        println!("Comfort scheduler: disabled (no [comfort].windows configured).");
        return Ok(());
    }

    // Enabled but not yet computable (missing temp/volume/forecast).
    if plan.get("available").and_then(|v| v.as_bool()) != Some(true) {
        let reason = plan
            .get("reason")
            .and_then(|v| v.as_str())
            .unwrap_or("plan not available yet");
        println!("Comfort scheduler: enabled, but no plan yet ({reason}).");
        return Ok(());
    }

    println!("Advisory pool comfort heat plan (READ-ONLY — actuates nothing)");
    if let Some(summary) = plan.get("summary").and_then(|v| v.as_str()) {
        println!("  {summary}");
    }
    println!();

    let f = |key: &str| plan.get(key).and_then(|v| v.as_f64()).unwrap_or(0.0);
    println!("Projected energy / cost (gas heater):");
    println!(
        "  Optimizer: {:.2} therms gas + {:.1} kWh pump / ${:.2}",
        f("optimizer_gas_therms"),
        f("optimizer_pump_kwh"),
        f("optimizer_usd")
    );
    println!(
        "  Baseline:  {:.2} therms gas + {:.1} kWh pump / ${:.2}  (constant {:.0}°F setpoint)",
        f("baseline_gas_therms"),
        f("baseline_pump_kwh"),
        f("baseline_usd"),
        f("baseline_setpoint_f"),
    );
    println!(
        "  Savings:   ${:.2}  ({:.0}%)",
        f("savings_usd"),
        f("savings_pct") * 100.0,
    );

    // Comfort outcomes per window.
    if let Some(outcomes) = plan.get("comfort_met").and_then(|v| v.as_array()) {
        if !outcomes.is_empty() {
            println!();
            println!("Comfort windows:");
            for o in outcomes {
                let start = o.get("window_start_unix").and_then(|v| v.as_i64()).unwrap_or(0);
                let target = o.get("target_f").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let met = o.get("met").and_then(|v| v.as_bool()).unwrap_or(false);
                println!(
                    "  start@{start}  target {target:.0}°F  -> {}",
                    if met { "MET" } else { "NOT met" }
                );
            }
        }
    }

    // The recommended on/off schedule (heated slots only, to keep it terse).
    if let Some(schedule) = plan.get("schedule").and_then(|v| v.as_array()) {
        let heated = schedule
            .iter()
            .filter(|s| s.get("heat_on").and_then(|v| v.as_bool()) == Some(true))
            .count();
        println!();
        println!(
            "Recommended heating: {} of {} slots on.",
            heated,
            schedule.len()
        );
    }

    println!();
    println!("(Advisory only. Apply manually if you wish — this tool changes nothing.)");

    Ok(())
}

// ─── Backtest (replay the weather log as the forecast) ──────────────────────

/// Default log path for the recorded monitor stream.
fn default_monitor_log() -> std::path::PathBuf {
    dirs_home()
        .join(".config")
        .join("pool-temp")
        .join("logs")
        .join("monitor.jsonl")
}

/// Home directory, falling back to `.` so the path is always well-formed.
fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}

/// One weather record extracted from a `monitor.jsonl` line.
struct WxRecord {
    ts_unix: i64,
    air_f: f64,
    wind_mph: Option<f64>,
    humidity_fraction: Option<f64>,
    cloud_fraction: Option<f64>,
    lat: f64,
    lon: f64,
    /// `weather.timezone` (UTC offset seconds), when present.
    utc_offset_seconds: Option<i64>,
    /// Pool last-reliable temperature, when present (used as the start temp).
    pool_temp_f: Option<f64>,
}

/// Parse one JSONL line into a [`WxRecord`], or `None` if it lacks usable
/// weather (no air temp / timestamp).
fn parse_record(line: &str) -> Option<WxRecord> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    let wx = v.get("weather")?;
    let ts_unix = wx
        .get("dt")
        .and_then(|d| d.as_i64())
        .or_else(|| v.get("ts").and_then(|t| t.as_i64()))?;
    let air_f = wx.get("main")?.get("temp")?.as_f64()?;
    let wind_mph = wx
        .get("wind")
        .and_then(|w| w.get("speed"))
        .and_then(|s| s.as_f64())
        .map(|s| s.max(0.0));
    let humidity_fraction = wx
        .get("main")
        .and_then(|m| m.get("humidity"))
        .and_then(|h| h.as_f64())
        .map(|h| (h / 100.0).clamp(0.0, 1.0));
    let cloud_fraction = wx
        .get("clouds")
        .and_then(|c| c.get("all"))
        .and_then(|a| a.as_f64())
        .map(|a| (a / 100.0).clamp(0.0, 1.0));
    let coord = wx.get("coord");
    let lat = coord.and_then(|c| c.get("lat")).and_then(|l| l.as_f64()).unwrap_or(0.0);
    let lon = coord.and_then(|c| c.get("lon")).and_then(|l| l.as_f64()).unwrap_or(0.0);
    let utc_offset_seconds = wx.get("timezone").and_then(|t| t.as_i64());
    let pool_temp_f = v
        .get("pool")
        .and_then(|p| p.get("pool"))
        .and_then(|p| p.get("last_reliable_temperature"))
        .and_then(|t| t.as_f64());

    Some(WxRecord {
        ts_unix,
        air_f,
        wind_mph,
        humidity_fraction,
        cloud_fraction,
        lat,
        lon,
        utc_offset_seconds,
        pool_temp_f,
    })
}

/// Run the backtest: read the monitor log, replay it as the forecast, and print
/// a model-projected savings report. Read-only; actuates nothing.
fn run_backtest(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let path = default_monitor_log();
    if !path.exists() {
        let msg = format!(
            "no weather history found at {} — run the monitor first to record one",
            path.display()
        );
        if json {
            output::print_json(&serde_json::json!({ "available": false, "reason": msg }));
        } else {
            println!("{msg}");
        }
        return Ok(());
    }

    let contents = std::fs::read_to_string(&path)?;
    let mut records: Vec<WxRecord> = contents
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(parse_record)
        .collect();
    records.sort_by_key(|r| r.ts_unix);
    records.dedup_by_key(|r| r.ts_unix);

    if records.len() < 2 {
        let msg = format!(
            "weather history at {} has too few usable records ({}) to backtest",
            path.display(),
            records.len()
        );
        if json {
            output::print_json(&serde_json::json!({ "available": false, "reason": msg }));
        } else {
            println!("{msg}");
        }
        return Ok(());
    }

    // Solar site + UTC offset from the (last) record; cover transmission uses the
    // spec's typical solar/heat-retention cover (~0.75).
    const COVER_SOLAR_TRANSMISSION: f64 = 0.75;
    let last = records.last().unwrap();
    let lat = last.lat;
    let lon = last.lon;
    let utc_offset_seconds = last.utc_offset_seconds.unwrap_or(-8 * 3600);

    // Build piecewise-constant segments: each record owns the span to the next
    // record's timestamp; the final record holds for one hour.
    let mut segments: Vec<WeatherSegment> = Vec::with_capacity(records.len());
    for i in 0..records.len() {
        let start = records[i].ts_unix * 1000;
        let end = if i + 1 < records.len() {
            records[i + 1].ts_unix * 1000
        } else {
            start + 3_600_000
        };
        if end <= start {
            continue;
        }
        segments.push(WeatherSegment {
            start_unix_ms: start,
            end_unix_ms: end,
            air_temp_f: records[i].air_f,
            wind_mph: records[i].wind_mph,
            humidity_fraction: records[i].humidity_fraction,
            cloud_fraction: records[i].cloud_fraction,
            latitude_deg: lat,
            longitude_deg: lon,
            cover_solar_transmission: COVER_SOLAR_TRANSMISSION,
        });
    }

    let anchor_unix = records.first().unwrap().ts_unix;
    let horizon_secs = last.ts_unix - anchor_unix;
    let horizon_hours = ((horizon_secs as f64) / 3600.0).max(1.0);

    // Start temperature: the first reliable pool temp in the log, else a sane
    // default. (The backtest is model-projected; the start anchors the sim.)
    let start_temp_f = records
        .iter()
        .find_map(|r| r.pool_temp_f)
        .unwrap_or(80.0);

    // Model + targets. The CLI has no config plumbing, so we use the spec's
    // defaults: spec-default gas heater, a flat gas price, a PG&E-like peak
    // electricity rate (costs the pump only), the example weekend comfort
    // window, and a default pool volume. These are CLEARLY documented in the
    // output so the numbers are honest model projections.
    let heater = GasHeaterModel::spec_default();
    let gas_rate = GasRate { usd_per_therm: 1.80 };
    let params = CoolingParams::seed(); // stands in for the fitted (k, g)
    let rates = RateSchedule {
        periods: vec![pentair_daemon::scheduler::RatePeriod {
            days: vec![
                Weekday::Mon,
                Weekday::Tue,
                Weekday::Wed,
                Weekday::Thu,
                Weekday::Fri,
                Weekday::Sat,
                Weekday::Sun,
            ],
            start: HhMm::new(16, 0),
            end: HhMm::new(21, 0),
            usd_per_kwh: 0.55,
        }],
        default_usd_per_kwh: 0.30,
        utc_offset_seconds,
    };
    let window = ComfortWindow {
        days: vec![Weekday::Sat, Weekday::Sun],
        start: HhMm::new(15, 0),
        end: HhMm::new(20, 0),
        target_f: 88.0,
    };
    const POOL_VOLUME_GALLONS: f64 = 20_000.0;
    let thermal_mass_btu_per_f = scheduler::thermal_mass_btu_per_f(POOL_VOLUME_GALLONS);

    let input = BacktestInput {
        segments: &segments,
        params: &params,
        heater: &heater,
        gas_rate: &gas_rate,
        rates: &rates,
        windows: std::slice::from_ref(&window),
        thermal_mass_btu_per_f,
        start_temp_f,
        anchor_unix,
        slot_hours: 1.0,
        horizon_hours,
        baseline_setpoint_f: window.target_f,
    };

    let bt = scheduler::run_backtest(&input);
    let report = &bt.report;

    if json {
        output::print_json(&serde_json::json!({
            "available": true,
            "advisory": true,
            "actuates": false,
            "energy_is_model_projected": bt.energy_is_model_projected,
            "source": path.display().to_string(),
            "anchor_unix": bt.anchor_unix,
            "slot_hours": bt.slot_hours,
            "num_slots": bt.num_slots,
            "start_temp_f": start_temp_f,
            "optimizer_gas_therms": report.optimizer_gas_therms,
            "optimizer_pump_kwh": report.optimizer_pump_kwh,
            "optimizer_usd": report.optimizer_usd,
            "baseline_gas_therms": report.baseline_gas_therms,
            "baseline_pump_kwh": report.baseline_pump_kwh,
            "baseline_usd": report.baseline_usd,
            "savings_usd": report.savings_usd,
            "savings_pct": report.savings_pct,
            "all_comfort_met": report.all_comfort_met(),
            "summary": report.summary,
        }));
        return Ok(());
    }

    let heated = report.schedule.iter().filter(|(_, on)| *on).count();
    println!("Pool comfort BACKTEST (READ-ONLY — actuates nothing)");
    println!("  Replayed weather: {}", path.display());
    println!(
        "  {} records over ~{:.0}h; assumptions: spec-default gas heater @ $1.80/therm, \
         pump on PG&E-like peak 16:00–21:00 @ $0.55 (else $0.30), Sat/Sun 15:00–20:00 @ 88°F, \
         {POOL_VOLUME_GALLONS:.0} gal pool, seed (k,g).",
        records.len(),
        horizon_hours
    );
    println!();
    println!("Projected energy / cost (MODEL-PROJECTED — no real energy meter):");
    println!(
        "  Optimizer: {:.2} therms gas + {:.1} kWh pump / ${:.2}",
        report.optimizer_gas_therms, report.optimizer_pump_kwh, report.optimizer_usd
    );
    println!(
        "  Baseline:  {:.2} therms gas + {:.1} kWh pump / ${:.2}  (constant {:.0}°F setpoint)",
        report.baseline_gas_therms, report.baseline_pump_kwh, report.baseline_usd, window.target_f
    );
    println!(
        "  Savings:   ${:.2}  ({:.0}%)",
        report.savings_usd,
        report.savings_pct * 100.0
    );
    println!();
    println!(
        "Recommended heating: {} of {} slots on.  Comfort: {}.",
        heated,
        bt.num_slots,
        if report.all_comfort_met() { "MET" } else { "NOT met" }
    );
    println!();
    println!("(Advisory only. Model-projected energy; this tool changes nothing.)");

    Ok(())
}
