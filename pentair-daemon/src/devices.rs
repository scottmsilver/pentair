use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DeviceStore {
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
                Ok(contents) => serde_json::from_str(&contents).unwrap_or_default(),
                Err(_) => DeviceStore::default(),
            }
        } else {
            DeviceStore::default()
        };
        info!(
            "loaded {} device token(s) from {:?}",
            store.tokens.len(),
            path
        );
        Self {
            store: Arc::new(RwLock::new(store)),
            path,
        }
    }

    pub async fn register(&self, token: String) -> Result<(), &'static str> {
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
        if !store.tokens.contains(&token) {
            store.tokens.push(token);
            self.persist(&store);
            info!("registered new device token ({} total)", store.tokens.len());
        }
        Ok(())
    }

    pub async fn remove(&self, token: &str) {
        let mut store = self.store.write().await;
        let before = store.tokens.len();
        store.tokens.retain(|t| t != token);
        if store.tokens.len() < before {
            self.persist(&store);
            info!(
                "removed invalid device token ({} remaining)",
                store.tokens.len()
            );
        }
    }

    pub async fn tokens(&self) -> Vec<String> {
        self.store.read().await.tokens.clone()
    }

    fn persist(&self, store: &DeviceStore) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(store) {
            Ok(json) => {
                if let Err(e) = std::fs::write(&self.path, json) {
                    warn!("failed to persist device tokens: {}", e);
                }
            }
            Err(e) => warn!("failed to serialize device tokens: {}", e),
        }
    }
}
