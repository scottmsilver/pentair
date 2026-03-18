use std::net::SocketAddr;
use std::time::Duration;
use tokio::net::UdpSocket;
use tracing::debug;

use pentair_protocol::responses::{parse_discovery, DiscoveryResponse};

use crate::error::{ClientError, Result};

const DISCOVERY_PORT: u16 = 1444;
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(3);
const DISCOVERY_REQUEST: &[u8] = &[0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];

/// Discover ScreenLogic adapters on the local network.
/// Sends a UDP broadcast and waits for responses.
/// Returns the first adapter found, or an error after timeout.
pub async fn discover() -> Result<DiscoveryResponse> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.set_broadcast(true)?;

    let target: SocketAddr = format!("255.255.255.255:{}", DISCOVERY_PORT)
        .parse()
        .unwrap();
    socket.send_to(DISCOVERY_REQUEST, target).await?;

    debug!("sent discovery broadcast to {}", target);

    let mut buf = [0u8; 256];
    match tokio::time::timeout(DISCOVERY_TIMEOUT, socket.recv_from(&mut buf)).await {
        Ok(Ok((n, from))) => {
            debug!("discovery response from {}: {} bytes", from, n);
            let resp = parse_discovery(&buf[..n])?;
            Ok(resp)
        }
        Ok(Err(e)) => Err(ClientError::Io(e)),
        Err(_) => Err(ClientError::DiscoveryFailed),
    }
}

/// Discover all adapters, waiting the full timeout period.
pub async fn discover_all() -> Result<Vec<DiscoveryResponse>> {
    let socket = UdpSocket::bind("0.0.0.0:0").await?;
    socket.set_broadcast(true)?;

    let target: SocketAddr = format!("255.255.255.255:{}", DISCOVERY_PORT)
        .parse()
        .unwrap();
    socket.send_to(DISCOVERY_REQUEST, target).await?;

    let mut results = Vec::new();
    let mut buf = [0u8; 256];

    let deadline = tokio::time::Instant::now() + DISCOVERY_TIMEOUT;
    loop {
        match tokio::time::timeout_at(deadline, socket.recv_from(&mut buf)).await {
            Ok(Ok((n, _from))) => {
                if let Ok(resp) = parse_discovery(&buf[..n]) {
                    results.push(resp);
                }
            }
            _ => break,
        }
    }

    if results.is_empty() {
        Err(ClientError::DiscoveryFailed)
    } else {
        Ok(results)
    }
}
