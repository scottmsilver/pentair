use crate::config::ApnsConfig;
use crate::devices::DeviceManager;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::Serialize;
use std::time::SystemTime;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

// ── JWT claims for APNs ────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct ApnsClaims {
    iss: String,
    iat: u64,
}

// ── JWT cache (55 min lifetime, APNs tokens expire at 60 min) ──────────

struct JwtCache {
    token: Option<String>,
    expires_at: u64,
}

impl JwtCache {
    fn new() -> Self {
        Self {
            token: None,
            expires_at: 0,
        }
    }

    fn get(&self) -> Option<&str> {
        let now = now_secs();
        if now < self.expires_at {
            self.token.as_deref()
        } else {
            None
        }
    }

    fn set(&mut self, token: String, expires_at: u64) {
        self.token = Some(token);
        self.expires_at = expires_at;
    }

    fn clear(&mut self) {
        self.token = None;
        self.expires_at = 0;
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ── APNs sender ────────────────────────────────────────────────────────

pub struct ApnsSender {
    key_id: String,
    team_id: String,
    encoding_key: EncodingKey,
    bundle_id: String,
    base_url: String,
    jwt_cache: RwLock<JwtCache>,
    http: reqwest::Client,
    devices: DeviceManager,
}

impl ApnsSender {
    /// Create a new ApnsSender. Returns None if APNs is not configured.
    pub fn new(config: &ApnsConfig, devices: DeviceManager) -> Option<Self> {
        if config.key_id.is_empty()
            || config.team_id.is_empty()
            || config.key_path.is_empty()
            || config.bundle_id.is_empty()
        {
            info!("APNs not configured — live activity updates disabled");
            return None;
        }

        // Resolve ~ in path
        let key_path = if let Some(stripped) = config.key_path.strip_prefix("~/") {
            dirs::home_dir()
                .unwrap_or_else(|| std::path::PathBuf::from("."))
                .join(stripped)
        } else {
            std::path::PathBuf::from(&config.key_path)
        };

        let pem_bytes = match std::fs::read(&key_path) {
            Ok(bytes) => bytes,
            Err(e) => {
                error!("failed to read APNs key file '{:?}': {}", key_path, e);
                return None;
            }
        };

        let encoding_key = match EncodingKey::from_ec_pem(&pem_bytes) {
            Ok(key) => key,
            Err(e) => {
                error!("failed to parse APNs EC key: {}", e);
                return None;
            }
        };

        let base_url = if config.environment == "production" {
            "https://api.push.apple.com".to_string()
        } else {
            "https://api.development.push.apple.com".to_string()
        };

        info!(
            "APNs configured: team={}, key={}, bundle={}, env={}",
            config.team_id, config.key_id, config.bundle_id, config.environment
        );

        Some(Self {
            key_id: config.key_id.clone(),
            team_id: config.team_id.clone(),
            encoding_key,
            bundle_id: config.bundle_id.clone(),
            base_url,
            jwt_cache: RwLock::new(JwtCache::new()),
            http: reqwest::Client::builder()
                .build()
                .unwrap_or_else(|_| reqwest::Client::new()),
            devices,
        })
    }

    /// Get a valid JWT, refreshing if cached token is expired.
    async fn get_jwt(&self) -> Result<String, String> {
        // Check cache first
        {
            let cache = self.jwt_cache.read().await;
            if let Some(token) = cache.get() {
                return Ok(token.to_string());
            }
        }

        // Sign a new JWT
        let now = now_secs();
        let claims = ApnsClaims {
            iss: self.team_id.clone(),
            iat: now,
        };

        let mut header = Header::new(Algorithm::ES256);
        header.kid = Some(self.key_id.clone());

        let jwt = encode(&header, &claims, &self.encoding_key)
            .map_err(|e| format!("APNs JWT signing failed: {}", e))?;

        // Cache for 55 minutes (APNs tokens expire at 60 min)
        {
            let mut cache = self.jwt_cache.write().await;
            cache.set(jwt.clone(), now + 55 * 60);
        }

        Ok(jwt)
    }

    /// Send a live activity update to all registered live activity tokens.
    pub async fn send_live_activity_update(
        &self,
        content_state: &serde_json::Value,
        is_milestone: bool,
    ) {
        let tokens = self.devices.live_activity_tokens().await;
        if tokens.is_empty() {
            return;
        }

        let now = now_secs();
        let mut payload = serde_json::json!({
            "aps": {
                "timestamp": now,
                "event": "update",
                "content-state": content_state
            }
        });

        if is_milestone {
            payload["aps"]["sound"] = serde_json::json!("default");
        }

        let priority = if is_milestone { 10 } else { 5 };

        info!(
            "APNs: sending live activity update to {} token(s), milestone={}",
            tokens.len(),
            is_milestone
        );

        for token in &tokens {
            self.send_to_token(token, &payload, priority).await;
        }
    }

    /// Send a live activity end event to all registered live activity tokens.
    pub async fn send_live_activity_end(&self, content_state: &serde_json::Value) {
        let tokens = self.devices.live_activity_tokens().await;
        if tokens.is_empty() {
            return;
        }

        let now = now_secs();
        let dismissal_date = now + 30;
        let payload = serde_json::json!({
            "aps": {
                "timestamp": now,
                "event": "end",
                "dismissal-date": dismissal_date,
                "sound": "default",
                "content-state": content_state
            }
        });

        info!("APNs: ending live activity for {} token(s)", tokens.len());

        for token in &tokens {
            self.send_to_token(token, &payload, 10).await;
        }
    }

    /// Send a payload to a single APNs token with retry on auth failure.
    async fn send_to_token(&self, token: &str, payload: &serde_json::Value, priority: u8) {
        let result = self.do_send(token, payload, priority).await;
        match result {
            Ok(status) if status == 403 => {
                // Auth failure — clear JWT cache and retry once
                warn!("APNs: auth failure (403), clearing JWT cache and retrying");
                {
                    let mut cache = self.jwt_cache.write().await;
                    cache.clear();
                }
                let _ = self.do_send(token, payload, priority).await;
            }
            _ => {}
        }
    }

    /// Perform the actual HTTP/2 POST. Returns the status code on success.
    async fn do_send(
        &self,
        token: &str,
        payload: &serde_json::Value,
        priority: u8,
    ) -> Result<u16, String> {
        let jwt = self.get_jwt().await?;
        let url = format!("{}/3/device/{}", self.base_url, token);
        let topic = format!("{}.push-type.liveactivity", self.bundle_id);

        let result = self
            .http
            .post(&url)
            .header("authorization", format!("bearer {}", jwt))
            .header("apns-push-type", "liveactivity")
            .header("apns-topic", &topic)
            .header("apns-priority", priority.to_string())
            .json(payload)
            .send()
            .await;

        match result {
            Ok(resp) => {
                let status = resp.status().as_u16();
                match status {
                    200..=299 => Ok(status),
                    400 => {
                        let body = resp.text().await.unwrap_or_default();
                        warn!("APNs: bad request (400): {}", body);
                        Ok(status)
                    }
                    403 => {
                        let body = resp.text().await.unwrap_or_default();
                        error!("APNs: auth failure (403): {}", body);
                        Ok(status)
                    }
                    410 => {
                        warn!("APNs: token expired (410), removing live activity token");
                        self.devices.remove_live_activity_token(token).await;
                        Ok(status)
                    }
                    429 => {
                        warn!("APNs: rate limited (429), skipping");
                        Ok(status)
                    }
                    500..=599 => {
                        error!("APNs: server error ({})", status);
                        Ok(status)
                    }
                    _ => {
                        let body = resp.text().await.unwrap_or_default();
                        warn!("APNs: unexpected status {}: {}", status, body);
                        Ok(status)
                    }
                }
            }
            Err(e) => {
                error!("APNs: request failed: {}", e);
                Err(format!("APNs request failed: {}", e))
            }
        }
    }
}
