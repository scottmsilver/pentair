use crate::backend::Backend;
use crate::output;

pub async fn run(
    backend: &mut Backend,
    action: u16,
    payload_hex: Option<&str>,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    let payload = match payload_hex {
        Some(hex) => parse_hex(hex)?,
        None => Vec::new(),
    };

    let (header, response) = backend.send_raw_action(action, &payload).await?;

    if json {
        output::print_json(&serde_json::json!({
            "request_action": action,
            "request_payload": payload_hex.unwrap_or(""),
            "response_action": header.action,
            "response_length": response.len(),
            "response_hex": hex_encode(&response),
        }));
    } else {
        println!("Response action: {}", header.action);
        println!("Response length: {} bytes", response.len());
        if !response.is_empty() {
            println!("Response hex:    {}", hex_encode(&response));
        }
    }

    Ok(())
}

fn parse_hex(s: &str) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    // Strip optional 0x prefix and whitespace
    let cleaned: String = s
        .trim()
        .strip_prefix("0x")
        .unwrap_or(s)
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();

    if cleaned.len() % 2 != 0 {
        return Err("Hex string must have even number of characters".into());
    }

    let mut bytes = Vec::with_capacity(cleaned.len() / 2);
    for i in (0..cleaned.len()).step_by(2) {
        let byte = u8::from_str_radix(&cleaned[i..i + 2], 16)
            .map_err(|e| format!("Invalid hex at position {}: {}", i, e))?;
        bytes.push(byte);
    }

    Ok(bytes)
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| format!("{:02x}", b))
        .collect::<Vec<_>>()
        .join("")
}
