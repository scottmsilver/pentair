use crate::devices::DeviceManager;
use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::{error, info, warn};

// ─── Service account JSON structure ─────────────────────────────────────

#[derive(Debug, Deserialize)]
struct ServiceAccount {
    client_email: String,
    private_key: String,
    token_uri: String,
}

// ─── JWT claims for OAuth2 ──────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct Claims {
    iss: String,
    scope: String,
    aud: String,
    iat: u64,
    exp: u64,
}

// ─── Token cache ────────────────────────────────────────────────────────

struct TokenCache {
    token: Option<String>,
    expires_at: u64,
}

impl TokenCache {
    fn new() -> Self {
        Self {
            token: None,
            expires_at: 0,
        }
    }

    fn get(&self) -> Option<&str> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now < self.expires_at {
            self.token.as_deref()
        } else {
            None
        }
    }

    fn set(&mut self, token: String, expires_in_secs: u64) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.token = Some(token);
        self.expires_at = now + expires_in_secs;
    }
}

// ─── FCM message structures ─────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct FcmMessage {
    message: FcmMessageBody,
}

#[derive(Debug, Serialize)]
struct FcmMessageBody {
    token: String,
    notification: FcmNotification,
}

#[derive(Debug, Serialize)]
struct FcmNotification {
    title: String,
    body: String,
}

// ─── OAuth2 token response ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: u64,
}

// ─── FcmSender ──────────────────────────────────────────────────────────

pub struct FcmSender {
    project_id: String,
    service_account: ServiceAccount,
    token_cache: RwLock<TokenCache>,
    http: reqwest::Client,
    devices: DeviceManager,
}

impl FcmSender {
    /// Create a new FcmSender. Returns None if FCM is not configured
    /// (empty project_id or service_account path).
    pub fn new(
        project_id: String,
        service_account_path: &str,
        devices: DeviceManager,
    ) -> Option<Self> {
        if project_id.is_empty() || service_account_path.is_empty() {
            info!("FCM not configured — push notifications disabled");
            return None;
        }

        let sa_json = match std::fs::read_to_string(service_account_path) {
            Ok(json) => json,
            Err(e) => {
                error!("failed to read FCM service account file '{}': {}", service_account_path, e);
                return None;
            }
        };

        let service_account: ServiceAccount = match serde_json::from_str(&sa_json) {
            Ok(sa) => sa,
            Err(e) => {
                error!("failed to parse FCM service account JSON: {}", e);
                return None;
            }
        };

        info!("FCM configured for project '{}' with service account '{}'",
            project_id, service_account.client_email);

        Some(Self {
            project_id,
            service_account,
            token_cache: RwLock::new(TokenCache::new()),
            http: reqwest::Client::new(),
            devices,
        })
    }

    /// Get a valid OAuth2 access token, refreshing if needed.
    async fn get_access_token(&self) -> Result<String, String> {
        // Check cache first
        {
            let cache = self.token_cache.read().await;
            if let Some(token) = cache.get() {
                return Ok(token.to_string());
            }
        }

        // Sign a new JWT
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("system time error: {}", e))?
            .as_secs();

        let claims = Claims {
            iss: self.service_account.client_email.clone(),
            scope: "https://www.googleapis.com/auth/firebase.messaging".to_string(),
            aud: self.service_account.token_uri.clone(),
            iat: now,
            exp: now + 3600, // 1 hour
        };

        let key = EncodingKey::from_rsa_pem(self.service_account.private_key.as_bytes())
            .map_err(|e| format!("invalid RSA key: {}", e))?;

        let jwt = encode(&Header::new(Algorithm::RS256), &claims, &key)
            .map_err(|e| format!("JWT signing failed: {}", e))?;

        // Exchange JWT for access token
        let resp = self.http
            .post(&self.service_account.token_uri)
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &jwt),
            ])
            .send()
            .await
            .map_err(|e| format!("token request failed: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("token exchange failed ({}): {}", status, body));
        }

        let token_resp: TokenResponse = resp
            .json()
            .await
            .map_err(|e| format!("failed to parse token response: {}", e))?;

        // Cache with 55 min expiry (token lasts 60 min, refresh early)
        let cache_duration = if token_resp.expires_in > 300 {
            token_resp.expires_in - 300
        } else {
            55 * 60
        };

        let token = token_resp.access_token.clone();
        {
            let mut cache = self.token_cache.write().await;
            cache.set(token_resp.access_token, cache_duration);
        }

        Ok(token)
    }

    /// Send a push notification to all registered devices.
    pub async fn send(&self, title: &str, body: &str) {
        let tokens = self.devices.tokens().await;
        if tokens.is_empty() {
            return;
        }

        let access_token = match self.get_access_token().await {
            Ok(t) => t,
            Err(e) => {
                error!("FCM: failed to get access token: {}", e);
                return;
            }
        };

        let url = format!(
            "https://fcm.googleapis.com/v1/projects/{}/messages:send",
            self.project_id
        );

        info!("FCM: sending '{}' to {} device(s)", title, tokens.len());

        for token in &tokens {
            let message = FcmMessage {
                message: FcmMessageBody {
                    token: token.clone(),
                    notification: FcmNotification {
                        title: title.to_string(),
                        body: body.to_string(),
                    },
                },
            };

            let result = self.http
                .post(&url)
                .bearer_auth(&access_token)
                .json(&message)
                .send()
                .await;

            match result {
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    match status {
                        200..=299 => {}
                        404 | 410 => {
                            warn!("FCM: token invalid ({}), removing", status);
                            self.devices.remove(token).await;
                        }
                        401 => {
                            let body = resp.text().await.unwrap_or_default();
                            error!("FCM: auth error (401): {}", body);
                        }
                        429 => {
                            warn!("FCM: rate limited (429)");
                        }
                        500..=599 => {
                            error!("FCM: server error ({})", status);
                        }
                        _ => {
                            let body = resp.text().await.unwrap_or_default();
                            warn!("FCM: unexpected status {}: {}", status, body);
                        }
                    }
                }
                Err(e) => {
                    error!("FCM: request failed: {}", e);
                }
            }
        }
    }
}
