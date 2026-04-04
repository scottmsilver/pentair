use hmac::{Hmac, Mac};
use rand::Rng;
use sha2::Sha256;
use std::fs;
use std::path::PathBuf;

type HmacSha256 = Hmac<Sha256>;

fn secret_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".pentair")
        .join("network-secret")
}

pub fn load_or_create() -> String {
    let path = secret_path();
    if let Ok(secret) = fs::read_to_string(&path) {
        let secret = secret.trim().to_string();
        if secret.len() >= 32 {
            return secret;
        }
    }
    let secret: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Alphanumeric)
        .take(64)
        .map(char::from)
        .collect();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
    fs::write(&path, &secret).ok();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).ok();
    }
    tracing::info!("generated new network secret at {}", path.display());
    secret
}

pub fn sign(secret: &str, email: &str, timestamp: u64) -> String {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC key");
    mac.update(format!("{}|{}", email, timestamp).as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

