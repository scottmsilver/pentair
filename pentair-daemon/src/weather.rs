//! OpenWeather "Current 2.5" integration for the pool-temperature predictor.
//!
//! Two cooperating pieces:
//!
//! * [`WeatherClient`] performs the live HTTP fetch (air temp °F, wind mph,
//!   humidity %, clouds %) for a lat/lon. The API key is read **only** from the
//!   `OPENWEATHER_API_KEY` environment variable and the base URL defaults to the
//!   canonical OpenWeather host (override only via `OPENWEATHER_BASE_URL`). No
//!   key or URL is ever logged.
//! * [`WeatherCache`] is an in-memory ring buffer of recent observed samples plus
//!   a few forecast hours, persisted to a small JSON file alongside the
//!   heat-estimator store so a restart during an outage still has recent data.
//!   On an HTTP error the cache is left untouched, so the last-good samples keep
//!   serving (backoff is the caller's concern).
//!
//! The response→sample mapping, the ring/eviction policy, and the offline
//! fallback are all pure functions so the unit tests never touch the network.
//! The live HTTP path ([`WeatherClient::fetch_current`]) is the only impure
//! surface and is exercised only by the phase-3 loopback test, never by a unit
//! test.
//
// Wired into the adapter poll loop in phase 3; until then the public surface is
// exercised by the in-module unit tests, so suppress the dead-code lint.
#![allow(dead_code)]

use crate::thermal::{SolarSite, WeatherSegment};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{info, warn};

/// Whole-request timeout for a live weather fetch. Bounds how long the fetch can
/// take so it can never wedge the caller (the control loop spawns it detached,
/// but this is the hard ceiling).
const FETCH_TIMEOUT: Duration = Duration::from_secs(10);
/// Connection-establishment timeout for a live weather fetch.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(5);

/// Canonical OpenWeather host. This is the *default* literal; an operator may
/// only override it via the `OPENWEATHER_BASE_URL` env var. No alternate host is
/// ever hardcoded.
const DEFAULT_BASE_URL: &str = "https://api.openweathermap.org";

const MS_PER_HOUR: i64 = 3_600_000;

/// How long observed samples are retained in the ring buffer.
const OBSERVED_RETENTION_HOURS: i64 = 48;
/// How far into the future forecast samples are retained.
const FORECAST_HORIZON_HOURS: i64 = 12;
/// Hard cap on retained samples (bounds memory + the persisted file).
const MAX_SAMPLES: usize = 128;

/// Errors from a live weather fetch. Unit tests construct these directly to
/// exercise the offline-fallback path without a network.
///
/// SECURITY: the API key travels as the `appid` query parameter, so the full
/// request URL — and therefore a `reqwest::Error`'s `Display` — can contain the
/// secret. This enum deliberately carries only a *sanitized* classification of
/// the failure (an HTTP status or a coarse transport kind); it never stores or
/// renders the source error or the URL. See [`sanitize_reqwest_error`].
#[derive(Debug, thiserror::Error)]
pub enum WeatherError {
    #[error("OPENWEATHER_API_KEY not set")]
    MissingApiKey,
    /// A transport-level failure, reduced to a static kind that can never
    /// contain the request URL or the API key.
    #[error("weather transport error ({0})")]
    Transport(&'static str),
    #[error("weather endpoint returned status {0}")]
    Status(u16),
}

/// Reduce a `reqwest::Error` to a [`WeatherError`] carrying only a sanitized
/// classification. The source error is **never** stored or formatted, because
/// its `Display` can include the full request URL (which holds the `appid` API
/// key). Only `error.status()` and the boolean kind predicates are consulted.
fn sanitize_reqwest_error(error: &reqwest::Error) -> WeatherError {
    if let Some(status) = error.status() {
        return WeatherError::Status(status.as_u16());
    }
    let kind = if error.is_timeout() {
        "timeout"
    } else if error.is_connect() {
        "connect"
    } else if error.is_decode() {
        "decode"
    } else if error.is_body() {
        "body"
    } else if error.is_request() {
        "request"
    } else {
        "other"
    };
    WeatherError::Transport(kind)
}

/// One piecewise-constant weather observation (or forecast hour).
///
/// Decoupled from [`WeatherSegment`] so it can carry a timestamp + a
/// forecast flag and round-trip through the persisted JSON cache.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct WeatherSample {
    pub observed_at_unix_ms: i64,
    /// Outdoor air temperature in °F.
    pub air_temp_f: f64,
    /// Wind speed in mph (`None` disables wind + evaporation in the model).
    pub wind_mph: Option<f64>,
    /// Relative humidity as a fraction in `[0, 1]` (`None` disables evaporation).
    pub humidity_fraction: Option<f64>,
    /// Cloud cover as a fraction in `[0, 1]` (`None` disables the solar bump).
    pub cloud_fraction: Option<f64>,
    /// True when this came from the forecast endpoint rather than an observation.
    #[serde(default)]
    pub is_forecast: bool,
}

impl WeatherSample {
    /// Project this sample onto a [`WeatherSegment`] spanning `[start, end)`,
    /// stamping in the fixed-site solar parameters.
    fn to_segment(
        self,
        start_unix_ms: i64,
        end_unix_ms: i64,
        site: SolarSite,
    ) -> WeatherSegment {
        WeatherSegment {
            start_unix_ms,
            end_unix_ms,
            air_temp_f: self.air_temp_f,
            wind_mph: self.wind_mph,
            humidity_fraction: self.humidity_fraction,
            cloud_fraction: self.cloud_fraction,
            latitude_deg: site.latitude_deg,
            longitude_deg: site.longitude_deg,
            cover_solar_transmission: site.cover_solar_transmission,
        }
    }
}

// ─── OpenWeather Current/Forecast response shapes ──────────────────────────

/// Subset of the OpenWeather Current 2.5 response we consume (units=imperial).
#[derive(Debug, Clone, Deserialize)]
struct OpenWeatherCurrent {
    main: OwMain,
    #[serde(default)]
    wind: Option<OwWind>,
    #[serde(default)]
    clouds: Option<OwClouds>,
    /// Observation time, unix seconds.
    dt: i64,
}

/// Subset of the OpenWeather Forecast 2.5 response (5 day / 3 hour).
#[derive(Debug, Clone, Deserialize)]
struct OpenWeatherForecast {
    list: Vec<OpenWeatherCurrent>,
}

#[derive(Debug, Clone, Deserialize)]
struct OwMain {
    /// Temperature in °F (units=imperial).
    temp: f64,
    /// Relative humidity in percent.
    #[serde(default)]
    humidity: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct OwWind {
    /// Wind speed in mph (units=imperial).
    #[serde(default)]
    speed: Option<f64>,
}

#[derive(Debug, Clone, Deserialize)]
struct OwClouds {
    /// Cloudiness in percent.
    #[serde(default)]
    all: Option<f64>,
}

/// Map a parsed Current response into a [`WeatherSample`]. Pure: tested without
/// any HTTP.
fn current_to_sample(resp: &OpenWeatherCurrent, is_forecast: bool) -> WeatherSample {
    WeatherSample {
        observed_at_unix_ms: resp.dt.saturating_mul(1000),
        air_temp_f: resp.main.temp,
        // Absent wind on a current observation means calm, which the model can
        // still use (keeps the full-weather basis); only a missing humidity
        // degrades to cooling-only.
        wind_mph: Some(
            resp.wind
                .as_ref()
                .and_then(|w| w.speed)
                .unwrap_or(0.0)
                .max(0.0),
        ),
        humidity_fraction: resp.main.humidity.map(|h| (h / 100.0).clamp(0.0, 1.0)),
        cloud_fraction: resp
            .clouds
            .as_ref()
            .and_then(|c| c.all)
            .map(|c| (c / 100.0).clamp(0.0, 1.0)),
        is_forecast,
    }
}

// ─── WeatherClient ─────────────────────────────────────────────────────────

/// Live OpenWeather HTTP client. Constructed only when an API key is present;
/// a missing key returns `None` so startup is never broken.
pub struct WeatherClient {
    http: reqwest::Client,
    base_url: String,
    api_key: String,
    latitude: f64,
    longitude: f64,
}

impl WeatherClient {
    /// Build a client for `(latitude, longitude)`, reading the API key from
    /// `OPENWEATHER_API_KEY` and the base URL from `OPENWEATHER_BASE_URL`
    /// (default [`DEFAULT_BASE_URL`]). Returns `None` when the key is absent.
    pub fn from_env(latitude: f64, longitude: f64) -> Option<Self> {
        let api_key = match std::env::var("OPENWEATHER_API_KEY") {
            Ok(key) if !key.trim().is_empty() => key,
            _ => {
                info!("OPENWEATHER_API_KEY not set — weather prediction disabled");
                return None;
            }
        };
        let base_url = std::env::var("OPENWEATHER_BASE_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_string());

        // Explicit timeouts so a slow/stalled endpoint can never hang the fetch
        // (and, with the detached spawn in the adapter, never the control loop).
        // If a bounded client can't be built (e.g. TLS backend init failure),
        // disable weather rather than fall back to an UNBOUNDED client.
        let http = match reqwest::Client::builder()
            .timeout(FETCH_TIMEOUT)
            .connect_timeout(CONNECT_TIMEOUT)
            .build()
        {
            Ok(client) => client,
            Err(_) => return None,
        };

        Some(Self {
            http,
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key,
            latitude,
            longitude,
        })
    }

    /// Fetch the current weather and map it to a [`WeatherSample`].
    ///
    /// Impure (network). Never unit-tested against the live API; the phase-3
    /// loopback test injects a mock host via `OPENWEATHER_BASE_URL`.
    pub async fn fetch_current(&self) -> Result<WeatherSample, WeatherError> {
        let url = format!("{}/data/2.5/weather", self.base_url);
        let resp = self
            .http
            .get(&url)
            .query(&[
                ("lat", self.latitude.to_string()),
                ("lon", self.longitude.to_string()),
                ("appid", self.api_key.clone()),
                ("units", "imperial".to_string()),
            ])
            .send()
            .await
            .map_err(|error| sanitize_reqwest_error(&error))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(WeatherError::Status(status.as_u16()));
        }
        let body: OpenWeatherCurrent = resp
            .json()
            .await
            .map_err(|error| sanitize_reqwest_error(&error))?;
        Ok(current_to_sample(&body, false))
    }

    /// Fetch up to `limit` forecast hours from the 5-day/3-hour endpoint.
    pub async fn fetch_forecast(&self, limit: usize) -> Result<Vec<WeatherSample>, WeatherError> {
        let url = format!("{}/data/2.5/forecast", self.base_url);
        let resp = self
            .http
            .get(&url)
            .query(&[
                ("lat", self.latitude.to_string()),
                ("lon", self.longitude.to_string()),
                ("appid", self.api_key.clone()),
                ("units", "imperial".to_string()),
            ])
            .send()
            .await
            .map_err(|error| sanitize_reqwest_error(&error))?;
        let status = resp.status();
        if !status.is_success() {
            return Err(WeatherError::Status(status.as_u16()));
        }
        let body: OpenWeatherForecast = resp
            .json()
            .await
            .map_err(|error| sanitize_reqwest_error(&error))?;
        Ok(body
            .list
            .iter()
            .take(limit)
            .map(|entry| current_to_sample(entry, true))
            .collect())
    }
}

// ─── WeatherCache ──────────────────────────────────────────────────────────

/// In-memory ring buffer of recent weather samples, persisted to a small JSON
/// file. Holds ~48h of observed samples plus a few forecast hours.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeatherCache {
    samples: Vec<WeatherSample>,
}

impl WeatherCache {
    /// Load a cache from `path`, or an empty cache if missing/unreadable.
    pub fn load(path: &std::path::Path) -> Self {
        if path.exists() {
            match std::fs::read_to_string(path) {
                Ok(contents) => match serde_json::from_str::<Self>(&contents) {
                    Ok(cache) => {
                        info!(
                            "weather cache: loaded {} sample(s) from {:?}",
                            cache.samples.len(),
                            path
                        );
                        return cache;
                    }
                    Err(error) => {
                        warn!("weather cache: failed to parse {:?}: {}", path, error);
                    }
                },
                Err(error) => warn!("weather cache: failed to read {:?}: {}", path, error),
            }
        }
        Self::default()
    }

    /// Persist the cache to `path`, creating parent directories as needed.
    pub fn persist(&self, path: &std::path::Path) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(self) {
            Ok(json) => {
                if let Err(error) = std::fs::write(path, json) {
                    warn!("weather cache: failed to persist {:?}: {}", path, error);
                }
            }
            Err(error) => warn!("weather cache: failed to serialize: {}", error),
        }
    }

    /// Number of retained samples (observed + forecast).
    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    /// Apply the result of a live current fetch.
    ///
    /// On success the new observation is recorded and stale samples evicted; on
    /// any error the cache is left untouched so the last-good samples keep
    /// serving. Returns `true` when the cache was updated.
    pub fn ingest_current(
        &mut self,
        result: Result<WeatherSample, WeatherError>,
        now_unix_ms: i64,
    ) -> bool {
        match result {
            Ok(sample) => {
                self.record_observation(sample, now_unix_ms);
                true
            }
            Err(error) => {
                warn!("weather fetch failed, serving last-good cache: {}", error);
                false
            }
        }
    }

    /// Record an observed sample and prune stale entries.
    pub fn record_observation(&mut self, sample: WeatherSample, now_unix_ms: i64) {
        let mut sample = sample;
        sample.is_forecast = false;
        self.samples.push(sample);
        self.prune(now_unix_ms);
    }

    /// Replace the forecast hours wholesale, keeping observed samples, then prune.
    pub fn set_forecast(&mut self, forecast: Vec<WeatherSample>, now_unix_ms: i64) {
        self.samples.retain(|s| !s.is_forecast);
        for mut sample in forecast {
            sample.is_forecast = true;
            self.samples.push(sample);
        }
        self.prune(now_unix_ms);
    }

    /// Evict observed samples older than the retention window, forecast samples
    /// past the horizon, then bound the total count.
    fn prune(&mut self, now_unix_ms: i64) {
        let observed_floor = now_unix_ms - OBSERVED_RETENTION_HOURS * MS_PER_HOUR;
        let forecast_ceiling = now_unix_ms + FORECAST_HORIZON_HOURS * MS_PER_HOUR;
        self.samples.retain(|s| {
            if s.is_forecast {
                s.observed_at_unix_ms <= forecast_ceiling
            } else {
                s.observed_at_unix_ms >= observed_floor
            }
        });
        self.samples.sort_by_key(|s| s.observed_at_unix_ms);
        if self.samples.len() > MAX_SAMPLES {
            let excess = self.samples.len() - MAX_SAMPLES;
            self.samples.drain(0..excess);
        }
    }

    /// The most recent observed (non-forecast) sample, if any.
    pub fn latest_observation(&self) -> Option<&WeatherSample> {
        self.samples
            .iter()
            .filter(|s| !s.is_forecast)
            .max_by_key(|s| s.observed_at_unix_ms)
    }

    /// Build piecewise-constant hourly [`WeatherSegment`]s from the cache.
    ///
    /// Each sample owns the span from its timestamp to the next sample's
    /// timestamp; the final sample holds for one hour. The thermal projector
    /// clips these to the actual sensing gap.
    pub fn to_segments(&self, site: SolarSite) -> Vec<WeatherSegment> {
        let mut samples = self.samples.clone();
        samples.sort_by_key(|s| s.observed_at_unix_ms);
        let mut segments = Vec::with_capacity(samples.len());
        for i in 0..samples.len() {
            let start = samples[i].observed_at_unix_ms;
            let end = if i + 1 < samples.len() {
                samples[i + 1].observed_at_unix_ms
            } else {
                start + MS_PER_HOUR
            };
            if end <= start {
                continue;
            }
            segments.push(samples[i].to_segment(start, end, site));
        }
        segments
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HOUR_MS: i64 = MS_PER_HOUR;

    fn sample(at_ms: i64, air: f64, forecast: bool) -> WeatherSample {
        WeatherSample {
            observed_at_unix_ms: at_ms,
            air_temp_f: air,
            wind_mph: Some(3.0),
            humidity_fraction: Some(0.5),
            cloud_fraction: Some(0.4),
            is_forecast: forecast,
        }
    }

    // ----- response → sample mapping -----

    #[test]
    fn current_response_maps_to_sample() {
        let json = r#"{
            "main": { "temp": 72.5, "humidity": 60 },
            "wind": { "speed": 5.2 },
            "clouds": { "all": 40 },
            "dt": 1719600000
        }"#;
        let resp: OpenWeatherCurrent = serde_json::from_str(json).expect("parse");
        let s = current_to_sample(&resp, false);
        assert_eq!(s.observed_at_unix_ms, 1_719_600_000_000);
        assert_eq!(s.air_temp_f, 72.5);
        assert_eq!(s.wind_mph, Some(5.2));
        assert_eq!(s.humidity_fraction, Some(0.6));
        assert_eq!(s.cloud_fraction, Some(0.4));
        assert!(!s.is_forecast);
    }

    #[test]
    fn current_response_without_wind_defaults_to_calm() {
        // Missing wind → calm (Some(0)), so the full-weather basis is preserved.
        let json = r#"{ "main": { "temp": 68.0, "humidity": 80 }, "dt": 100 }"#;
        let resp: OpenWeatherCurrent = serde_json::from_str(json).expect("parse");
        let s = current_to_sample(&resp, false);
        assert_eq!(s.wind_mph, Some(0.0));
        assert_eq!(s.humidity_fraction, Some(0.8));
        assert_eq!(s.cloud_fraction, None);
    }

    #[test]
    fn forecast_response_maps_to_samples() {
        let json = r#"{
            "list": [
                { "main": { "temp": 70.0, "humidity": 50 }, "wind": { "speed": 1.0 }, "clouds": { "all": 10 }, "dt": 200 },
                { "main": { "temp": 71.0, "humidity": 55 }, "wind": { "speed": 2.0 }, "clouds": { "all": 20 }, "dt": 10800 },
                { "main": { "temp": 72.0, "humidity": 60 }, "wind": { "speed": 3.0 }, "clouds": { "all": 30 }, "dt": 21600 }
            ]
        }"#;
        let resp: OpenWeatherForecast = serde_json::from_str(json).expect("parse");
        let mapped: Vec<WeatherSample> = resp
            .list
            .iter()
            .take(2)
            .map(|e| current_to_sample(e, true))
            .collect();
        assert_eq!(mapped.len(), 2);
        assert!(mapped.iter().all(|s| s.is_forecast));
        assert_eq!(mapped[0].observed_at_unix_ms, 200_000);
        assert_eq!(mapped[1].air_temp_f, 71.0);
    }

    // ----- ring buffer / eviction -----

    #[test]
    fn cache_ring_evicts_old_observed_samples() {
        let now = 1_000 * HOUR_MS;
        let mut cache = WeatherCache::default();
        // Older than the 48h retention window.
        cache.record_observation(sample(now - 100 * HOUR_MS, 60.0, false), now);
        // Within the window.
        cache.record_observation(sample(now - 2 * HOUR_MS, 65.0, false), now);
        cache.record_observation(sample(now - HOUR_MS, 66.0, false), now);
        assert_eq!(cache.len(), 2, "100h-old sample should be evicted");
        assert!(cache
            .latest_observation()
            .is_some_and(|s| s.air_temp_f == 66.0));
    }

    #[test]
    fn cache_evicts_forecast_past_horizon() {
        let now = 1_000 * HOUR_MS;
        let mut cache = WeatherCache::default();
        cache.set_forecast(
            vec![
                sample(now + 3 * HOUR_MS, 70.0, true),
                sample(now + 100 * HOUR_MS, 71.0, true), // past the 12h horizon
            ],
            now,
        );
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn cache_capacity_is_bounded() {
        let now = MAX_SAMPLES as i64 * HOUR_MS * 4;
        let mut cache = WeatherCache::default();
        // Insert far more than MAX_SAMPLES, all within the retention window.
        for i in 0..(MAX_SAMPLES + 50) {
            let at = now - (i as i64) * (HOUR_MS / 4); // 15-min spacing, all < 48h old
            cache.record_observation(sample(at, 60.0 + i as f64 * 0.01, false), now);
        }
        assert!(cache.len() <= MAX_SAMPLES);
        // Newest sample is retained (oldest dropped).
        assert!(cache
            .latest_observation()
            .is_some_and(|s| s.observed_at_unix_ms == now));
    }

    #[test]
    fn set_forecast_replaces_only_forecast_samples() {
        let now = 1_000 * HOUR_MS;
        let mut cache = WeatherCache::default();
        cache.record_observation(sample(now - HOUR_MS, 65.0, false), now);
        cache.set_forecast(vec![sample(now + HOUR_MS, 70.0, true)], now);
        cache.set_forecast(vec![sample(now + 2 * HOUR_MS, 71.0, true)], now);
        // Observation survives; only one forecast remains.
        assert_eq!(cache.samples.iter().filter(|s| !s.is_forecast).count(), 1);
        assert_eq!(cache.samples.iter().filter(|s| s.is_forecast).count(), 1);
    }

    // ----- offline fallback -----

    #[test]
    fn offline_fetch_keeps_last_good_cache() {
        let now = 1_000 * HOUR_MS;
        let mut cache = WeatherCache::default();
        let updated = cache.ingest_current(Ok(sample(now - HOUR_MS, 64.0, false)), now);
        assert!(updated);
        let before = cache.len();

        // Simulate an HTTP failure: cache must be unchanged and still serve.
        let updated = cache.ingest_current(Err(WeatherError::Status(503)), now);
        assert!(!updated);
        assert_eq!(cache.len(), before);
        assert!(
            !cache.to_segments(SolarSite::disabled()).is_empty(),
            "last-good still served"
        );
    }

    // ----- segment building -----

    #[test]
    fn to_segments_are_contiguous_and_hourly() {
        let now = 1_000 * HOUR_MS;
        let mut cache = WeatherCache::default();
        cache.record_observation(sample(now - 2 * HOUR_MS, 65.0, false), now);
        cache.record_observation(sample(now - HOUR_MS, 66.0, false), now);
        let segs = cache.to_segments(SolarSite::disabled());
        assert_eq!(segs.len(), 2);
        // First segment ends where the second begins (contiguous).
        assert_eq!(segs[0].end_unix_ms, segs[1].start_unix_ms);
        // Last segment holds for one hour.
        assert_eq!(segs[1].end_unix_ms - segs[1].start_unix_ms, HOUR_MS);
        // Weather fields carried through.
        assert_eq!(segs[0].air_temp_f, 65.0);
        assert_eq!(segs[1].humidity_fraction, Some(0.5));
    }

    // ----- persistence round-trip -----

    #[test]
    fn persist_round_trips_through_json() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("weather-cache.json");
        let now = 1_000 * HOUR_MS;
        let mut cache = WeatherCache::default();
        cache.record_observation(sample(now - HOUR_MS, 67.0, false), now);
        cache.set_forecast(vec![sample(now + HOUR_MS, 70.0, true)], now);
        cache.persist(&path);

        let loaded = WeatherCache::load(&path);
        assert_eq!(loaded.len(), cache.len());
        assert_eq!(
            loaded.latest_observation().map(|s| s.air_temp_f),
            Some(67.0)
        );
    }

    #[test]
    fn load_missing_file_yields_empty_cache() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("does-not-exist.json");
        let cache = WeatherCache::load(&path);
        assert!(cache.is_empty());
    }
}
