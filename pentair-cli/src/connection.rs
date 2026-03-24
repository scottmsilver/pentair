use pentair_client::client::Client;
use pentair_client::discovery::discover;

use crate::backend::Backend;

/// Resolve the adapter address: --host flag / PENTAIR_HOST env -> auto-discovery.
/// Then connect, performing the full login handshake.
/// Returns a Backend::Direct wrapping the TCP client.
pub async fn resolve_and_connect(
    host: Option<&str>,
) -> Result<Backend, Box<dyn std::error::Error>> {
    let addr = match host {
        Some(h) => {
            // If no port specified, default to :80
            if h.contains(':') {
                h.to_string()
            } else {
                format!("{}:80", h)
            }
        }
        None => {
            // Auto-discover
            eprintln!("Discovering adapters...");
            let resp = discover().await?;
            let addr = format!(
                "{}.{}.{}.{}:{}",
                resp.ip[0], resp.ip[1], resp.ip[2], resp.ip[3], resp.port
            );
            eprintln!("Found adapter at {}", addr);
            addr
        }
    };

    let client = Client::connect(&addr).await?;
    Ok(Backend::Direct(client))
}
