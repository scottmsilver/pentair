use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

// ── Per-device record ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct DeviceRecord {
    pub fcm_token: String,
    #[serde(default = "default_platform")]
    pub platform: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub live_activity_token: Option<String>,
}

fn default_platform() -> String {
    "unknown".to_string()
}

// ── Store format (new) ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DeviceStore {
    devices: Vec<DeviceRecord>,
}

// ── Legacy format (for migration) ──────────────────────────────────────

#[derive(Debug, Deserialize)]
struct LegacyStore {
    tokens: Vec<String>,
}

#[derive(Clone)]
pub struct DeviceManager {
    store: Arc<RwLock<DeviceStore>>,
    path: PathBuf,
}

impl DeviceManager {
    pub fn load(path: PathBuf) -> Self {
        let store = if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(contents) => match serde_json::from_str::<DeviceStore>(&contents) {
                    Ok(s) => s,
                    Err(_) => {
                        // Try legacy format migration
                        match serde_json::from_str::<LegacyStore>(&contents) {
                            Ok(legacy) => {
                                info!(
                                    "migrating {} legacy device token(s) to new format",
                                    legacy.tokens.len()
                                );
                                let devices = legacy
                                    .tokens
                                    .into_iter()
                                    .map(|token| DeviceRecord {
                                        fcm_token: token,
                                        platform: "unknown".to_string(),
                                        live_activity_token: None,
                                    })
                                    .collect();
                                DeviceStore { devices }
                            }
                            Err(_) => DeviceStore::default(),
                        }
                    }
                },
                Err(_) => DeviceStore::default(),
            }
        } else {
            DeviceStore::default()
        };
        info!(
            "loaded {} device(s) from {:?}",
            store.devices.len(),
            path
        );
        let mgr = Self {
            store: Arc::new(RwLock::new(store)),
            path,
        };
        // Persist after load to save any migration
        {
            let store_ref = mgr.store.clone();
            let path = mgr.path.clone();
            tokio::spawn(async move {
                let store = store_ref.read().await;
                persist_inner(&path, &store);
            });
        }
        mgr
    }

    /// Register or update a device. If the FCM token already exists, update
    /// platform and live_activity_token fields. Otherwise add a new record.
    pub async fn register(
        &self,
        token: String,
        platform: Option<String>,
        live_activity_token: Option<String>,
    ) -> Result<(), &'static str> {
        let trimmed = token.trim();
        if trimmed.is_empty() {
            warn!("rejected empty device token");
            return Err("token must not be empty");
        }
        if trimmed.len() < 10 {
            warn!("rejected too-short device token (len={})", trimmed.len());
            return Err("token too short");
        }
        let token = trimmed.to_string();
        let mut store = self.store.write().await;
        if let Some(existing) = store.devices.iter_mut().find(|d| d.fcm_token == token) {
            // Update existing device
            if let Some(p) = platform {
                existing.platform = p;
            }
            if let Some(lat) = live_activity_token {
                existing.live_activity_token = Some(lat);
            }
            self.persist(&store);
            info!("updated device registration ({} total)", store.devices.len());
        } else {
            store.devices.push(DeviceRecord {
                fcm_token: token,
                platform: platform.unwrap_or_else(|| "unknown".to_string()),
                live_activity_token,
            });
            self.persist(&store);
            info!("registered new device ({} total)", store.devices.len());
        }
        Ok(())
    }

    pub async fn remove(&self, token: &str) {
        let mut store = self.store.write().await;
        let before = store.devices.len();
        store.devices.retain(|d| d.fcm_token != token);
        if store.devices.len() < before {
            self.persist(&store);
            info!(
                "removed invalid device token ({} remaining)",
                store.devices.len()
            );
        }
    }

    /// Remove a live activity token (e.g. on APNs 410).
    pub async fn remove_live_activity_token(&self, la_token: &str) {
        let mut store = self.store.write().await;
        let mut changed = false;
        for device in &mut store.devices {
            if device.live_activity_token.as_deref() == Some(la_token) {
                device.live_activity_token = None;
                changed = true;
            }
        }
        if changed {
            self.persist(&store);
            info!("removed live activity token");
        }
    }

    /// Get all FCM tokens (for sending FCM messages to all devices).
    pub async fn tokens(&self) -> Vec<String> {
        self.store
            .read()
            .await
            .devices
            .iter()
            .map(|d| d.fcm_token.clone())
            .collect()
    }

    /// Get FCM tokens for Android devices only (platform != "ios").
    /// Used for data-only continuous updates where iOS uses APNs live activities instead.
    pub async fn android_tokens(&self) -> Vec<String> {
        self.store
            .read()
            .await
            .devices
            .iter()
            .filter(|d| d.platform != "ios")
            .map(|d| d.fcm_token.clone())
            .collect()
    }

    /// Get all live activity tokens (for sending APNs live activity updates).
    pub async fn live_activity_tokens(&self) -> Vec<String> {
        self.store
            .read()
            .await
            .devices
            .iter()
            .filter_map(|d| d.live_activity_token.clone())
            .collect()
    }

    fn persist(&self, store: &DeviceStore) {
        persist_inner(&self.path, store);
    }
}

fn persist_inner(path: &PathBuf, store: &DeviceStore) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match serde_json::to_string_pretty(store) {
        Ok(json) => {
            if let Err(e) = std::fs::write(path, json) {
                warn!("failed to persist device store: {}", e);
            }
        }
        Err(e) => warn!("failed to serialize device store: {}", e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[tokio::test]
    async fn migrate_legacy_flat_format() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"tokens":["abc123token","def456token"]}}"#).unwrap();

        let mgr = DeviceManager::load(f.path().to_path_buf());
        let tokens = mgr.tokens().await;
        assert_eq!(tokens.len(), 2);
        assert!(tokens.contains(&"abc123token".to_string()));
        assert!(tokens.contains(&"def456token".to_string()));

        // All migrated devices should have platform "unknown" and no LA token
        let la_tokens = mgr.live_activity_tokens().await;
        assert!(la_tokens.is_empty());
    }

    #[tokio::test]
    async fn load_new_format() {
        let mut f = NamedTempFile::new().unwrap();
        write!(
            f,
            r#"{{"devices":[{{"fcm_token":"abc","platform":"ios","live_activity_token":"xyz"}}]}}"#
        )
        .unwrap();

        let mgr = DeviceManager::load(f.path().to_path_buf());
        let tokens = mgr.tokens().await;
        assert_eq!(tokens, vec!["abc"]);

        let la_tokens = mgr.live_activity_tokens().await;
        assert_eq!(la_tokens, vec!["xyz"]);
    }

    #[tokio::test]
    async fn register_new_device() {
        let f = NamedTempFile::new().unwrap();
        let mgr = DeviceManager::load(f.path().to_path_buf());

        mgr.register("token12345".to_string(), Some("android".to_string()), None)
            .await
            .unwrap();

        let tokens = mgr.tokens().await;
        assert_eq!(tokens, vec!["token12345"]);
    }

    #[tokio::test]
    async fn register_updates_existing_device() {
        let f = NamedTempFile::new().unwrap();
        let mgr = DeviceManager::load(f.path().to_path_buf());

        mgr.register("token12345".to_string(), Some("ios".to_string()), None)
            .await
            .unwrap();
        mgr.register(
            "token12345".to_string(),
            None,
            Some("la-token-abc".to_string()),
        )
        .await
        .unwrap();

        let tokens = mgr.tokens().await;
        assert_eq!(tokens.len(), 1);

        let la_tokens = mgr.live_activity_tokens().await;
        assert_eq!(la_tokens, vec!["la-token-abc"]);
    }

    #[tokio::test]
    async fn remove_live_activity_token() {
        let f = NamedTempFile::new().unwrap();
        let mgr = DeviceManager::load(f.path().to_path_buf());

        mgr.register(
            "token12345".to_string(),
            Some("ios".to_string()),
            Some("la-token".to_string()),
        )
        .await
        .unwrap();

        assert_eq!(mgr.live_activity_tokens().await.len(), 1);

        mgr.remove_live_activity_token("la-token").await;
        assert!(mgr.live_activity_tokens().await.is_empty());
    }
}
