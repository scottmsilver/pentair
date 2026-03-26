use crate::pool_types::PoolSystem;

#[derive(Clone)]
pub struct DaemonClient {
    base_url: String,
    http: reqwest::Client,
}

#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    #[error("daemon unreachable: {0}")]
    Unreachable(#[from] reqwest::Error),
    #[error("daemon returned error: {status} {body}")]
    ApiError { status: u16, body: String },
}

impl DaemonClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            http: reqwest::Client::new(),
        }
    }

    pub async fn get_pool(&self) -> Result<PoolSystem, DaemonError> {
        let resp = self.http.get(format!("{}/api/pool", self.base_url)).send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DaemonError::ApiError { status, body });
        }
        Ok(resp.json().await?)
    }

    pub async fn post(&self, path: &str, body: Option<serde_json::Value>) -> Result<(), DaemonError> {
        let url = format!("{}{}", self.base_url, path);
        let mut req = self.http.post(&url);
        if let Some(b) = body {
            req = req.json(&b);
        }
        let resp = req.send().await?;
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(DaemonError::ApiError { status, body });
        }
        Ok(())
    }

    pub fn ws_url(&self) -> String {
        let ws_base = self.base_url
            .replace("http://", "ws://")
            .replace("https://", "wss://");
        format!("{}/api/ws", ws_base)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ws_url_from_http() {
        let client = DaemonClient::new("http://localhost:8080");
        assert_eq!(client.ws_url(), "ws://localhost:8080/api/ws");
    }

    #[test]
    fn ws_url_strips_trailing_slash() {
        let client = DaemonClient::new("http://localhost:8080/");
        assert_eq!(client.ws_url(), "ws://localhost:8080/api/ws");
    }
}
