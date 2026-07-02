//! Continuous thermal calibrator — pure core (spec:
//! docs/2026-07-01-thermal-calibrator-v1.md, Phases 1+2).
//!
//! Everything here is pure: no I/O, no clock, no store access. `heat.rs` owns
//! capture/persist/orchestration and calls into this module, mirroring how the
//! scheduler consumes `thermal.rs`.
//
// Tasks 1-3 land the pure primitives + tests; heat.rs wires them in from
// Task 6. Until then the binary target sees the public surface as unused, so
// suppress dead-code (same pattern scheduler.rs used during its phase-in).
#![allow(dead_code)]

use serde::{Deserialize, Serialize};

use crate::thermal::{SolarSite, WeatherSegment};

/// Hard cap on weather buckets stored per interval (spec §11): hourly buckets,
/// widened (never truncated) when the gap exceeds 24h, so the store stays small
/// while the whole gap remains covered.
pub const MAX_BUCKETS_PER_INTERVAL: usize = 24;

const MS_PER_HOUR: i64 = 3_600_000;

/// One summarized slice of the weather an interval experienced. Same fields as
/// a `WeatherSegment` minus the site (the site is global, re-attached on read).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WeatherBucket {
    pub start_unix_ms: i64,
    pub end_unix_ms: i64,
    pub air_temp_f: f64,
    pub wind_mph: Option<f64>,
    pub humidity_fraction: Option<f64>,
    pub cloud_fraction: Option<f64>,
}

/// Classification of a stored interval (spec §3). Heating / contaminated
/// intervals are never stored, so only these two regimes exist on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IntervalRegime {
    IdleCovered,
    ExcludedAnomalous,
}

/// A self-contained cooling interval (spec §11): two settled reliable reads
/// bounding an idle gap, plus the weather that spanned it. Self-contained so
/// the fit never depends on the 48h `WeatherCache` retention.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CoolingInterval {
    pub t0_unix_ms: i64,
    pub t1_unix_ms: i64,
    pub temp0_f: f64,
    pub temp1_f: f64,
    pub regime: IntervalRegime,
    pub weather: Vec<WeatherBucket>,
}

/// Summarize `segments` over `[t0, t1]` into at most [`MAX_BUCKETS_PER_INTERVAL`]
/// equal buckets (hourly when the gap is <= 24h). Each bucket copies the weather
/// of the segment covering its midpoint (nearest by start when none covers it).
/// Empty input -> empty output (the caller then skips capture).
pub fn bucket_weather(segments: &[WeatherSegment], t0_unix_ms: i64, t1_unix_ms: i64) -> Vec<WeatherBucket> {
    if segments.is_empty() || t1_unix_ms <= t0_unix_ms {
        return Vec::new();
    }
    let gap_ms = t1_unix_ms - t0_unix_ms;
    let n = ((gap_ms + MS_PER_HOUR - 1) / MS_PER_HOUR).clamp(1, MAX_BUCKETS_PER_INTERVAL as i64) as usize;
    let mut buckets = Vec::with_capacity(n);
    for i in 0..n {
        let start = t0_unix_ms + gap_ms * i as i64 / n as i64;
        let end = t0_unix_ms + gap_ms * (i as i64 + 1) / n as i64;
        let mid = start + (end - start) / 2;
        let seg = segments
            .iter()
            .find(|s| s.start_unix_ms <= mid && mid < s.end_unix_ms)
            .or_else(|| segments.iter().min_by_key(|s| (s.start_unix_ms - mid).abs()));
        let Some(seg) = seg else { continue };
        buckets.push(WeatherBucket {
            start_unix_ms: start,
            end_unix_ms: end,
            air_temp_f: seg.air_temp_f,
            wind_mph: seg.wind_mph,
            humidity_fraction: seg.humidity_fraction,
            cloud_fraction: seg.cloud_fraction,
        });
    }
    buckets
}

/// Re-attach the (global) solar site to a stored interval's buckets, yielding
/// segments the thermal relaxation can consume.
pub fn interval_segments(interval: &CoolingInterval, site: SolarSite) -> Vec<WeatherSegment> {
    interval
        .weather
        .iter()
        .map(|b| WeatherSegment {
            start_unix_ms: b.start_unix_ms,
            end_unix_ms: b.end_unix_ms,
            air_temp_f: b.air_temp_f,
            wind_mph: b.wind_mph,
            humidity_fraction: b.humidity_fraction,
            cloud_fraction: b.cloud_fraction,
            latitude_deg: site.latitude_deg,
            longitude_deg: site.longitude_deg,
            cover_solar_transmission: site.cover_solar_transmission,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thermal::{SolarSite, WeatherSegment};

    fn seg(start_h: i64, end_h: i64, air: f64) -> WeatherSegment {
        WeatherSegment {
            start_unix_ms: start_h * 3_600_000,
            end_unix_ms: end_h * 3_600_000,
            air_temp_f: air,
            wind_mph: Some(4.0),
            humidity_fraction: Some(0.6),
            cloud_fraction: Some(0.1),
            latitude_deg: 0.0,
            longitude_deg: 0.0,
            cover_solar_transmission: 0.0,
        }
    }

    #[test]
    fn buckets_are_hourly_and_capped_at_24() {
        // 10h gap -> 10 buckets; 40h gap -> capped at 24 wider buckets.
        let segs = vec![seg(0, 48, 60.0)];
        let ten = bucket_weather(&segs, 0, 10 * 3_600_000);
        assert_eq!(ten.len(), 10);
        assert_eq!(ten[0].start_unix_ms, 0);
        assert_eq!(ten[9].end_unix_ms, 10 * 3_600_000);
        let forty = bucket_weather(&segs, 0, 40 * 3_600_000);
        assert_eq!(forty.len(), MAX_BUCKETS_PER_INTERVAL);
        assert_eq!(forty.last().unwrap().end_unix_ms, 40 * 3_600_000);
    }

    #[test]
    fn buckets_take_weather_from_covering_segment() {
        // Air steps 60 -> 50 at hour 2; buckets on each side see their side.
        let segs = vec![seg(0, 2, 60.0), seg(2, 6, 50.0)];
        let buckets = bucket_weather(&segs, 0, 4 * 3_600_000);
        assert_eq!(buckets[0].air_temp_f, 60.0);
        assert_eq!(buckets[3].air_temp_f, 50.0);
    }

    #[test]
    fn empty_segments_yield_no_buckets() {
        assert!(bucket_weather(&[], 0, 3_600_000).is_empty());
    }

    #[test]
    fn interval_segments_round_trip_site() {
        let segs = vec![seg(0, 6, 55.0)];
        let interval = CoolingInterval {
            t0_unix_ms: 0,
            t1_unix_ms: 4 * 3_600_000,
            temp0_f: 90.0,
            temp1_f: 89.0,
            regime: IntervalRegime::IdleCovered,
            weather: bucket_weather(&segs, 0, 4 * 3_600_000),
        };
        let site = SolarSite { latitude_deg: 37.35, longitude_deg: -122.09, cover_solar_transmission: 0.75 };
        let out = interval_segments(&interval, site);
        assert_eq!(out.len(), interval.weather.len());
        assert_eq!(out[0].latitude_deg, 37.35);
        assert_eq!(out[0].air_temp_f, 55.0);
    }
}
