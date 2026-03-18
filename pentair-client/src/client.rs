use pentair_protocol::action::Action;
use pentair_protocol::codec::{decode_header, encode_message, MessageHeader, HEADER_SIZE};
use pentair_protocol::requests::*;
use pentair_protocol::responses::*;
use tokio::io::{AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};
use tokio::net::TcpStream;
use tracing::debug;

use crate::error::{ClientError, Result};

pub struct Client {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
    client_id: i32,
}

impl Client {
    /// Connect to a ScreenLogic adapter at the given address.
    /// Performs: TCP connect -> send CONNECTSERVERHOST -> login (challenge->login->addClient)
    pub async fn connect(addr: &str) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        let (read_half, mut write_half) = stream.into_split();
        let reader = BufReader::new(read_half);

        // Send connection string (raw, no framing).
        // The adapter silently accepts this and waits for framed messages —
        // it does NOT echo anything back.
        write_half.write_all(CONNECT_STRING).await?;

        let mut client = Client {
            reader,
            writer: write_half,
            client_id: rand_client_id(),
        };

        // Login sequence
        client.login().await?;

        Ok(client)
    }

    async fn login(&mut self) -> Result<()> {
        // 1. Challenge (optional, but send it)
        self.send_raw(build_challenge_request()).await?;
        let (_header, _payload) = self.recv_message().await?;

        // 2. Login
        self.send_raw(build_login_request()).await?;
        let (header, _payload) = self.recv_message().await?;
        if matches!(Action::try_from(header.action), Ok(Action::LoginFailure)) {
            return Err(ClientError::LoginFailed);
        }

        // 3. AddClient
        self.send_raw(build_add_client(self.client_id)).await?;
        let (_header, _payload) = self.recv_message().await?;

        Ok(())
    }

    /// Send a raw message (already framed with header).
    async fn send_raw(&mut self, msg: Vec<u8>) -> Result<()> {
        self.writer.write_all(&msg).await?;
        self.writer.flush().await?;
        Ok(())
    }

    /// Read one complete message from the adapter.
    /// Returns (header, payload).
    async fn recv_message(&mut self) -> Result<(MessageHeader, Vec<u8>)> {
        // Read 8-byte header
        let mut header_buf = [0u8; HEADER_SIZE];
        self.reader.read_exact(&mut header_buf).await?;
        let header = decode_header(&header_buf)?;

        // Read payload
        let mut payload = vec![0u8; header.data_length as usize];
        if !payload.is_empty() {
            self.reader.read_exact(&mut payload).await?;
        }

        Ok((header, payload))
    }

    /// Send a message and receive the response, skipping any push messages (12500-12505).
    /// Push messages have action codes in the range 12500-12505 and 9806.
    async fn send_and_recv(&mut self, msg: Vec<u8>) -> Result<(MessageHeader, Vec<u8>)> {
        self.send_raw(msg).await?;
        loop {
            let (header, payload) = self.recv_message().await?;
            if is_push_message(header.action) {
                debug!("skipping push message: action={}", header.action);
                continue;
            }
            return Ok((header, payload));
        }
    }

    // ── High-level command methods ─────────────────────────────────────────

    pub async fn get_version(&mut self) -> Result<VersionResponse> {
        let (_, payload) = self.send_and_recv(build_get_version()).await?;
        Ok(parse_version(&payload)?)
    }

    pub async fn get_status(&mut self) -> Result<PoolStatus> {
        let (_, payload) = self.send_and_recv(build_get_status()).await?;
        Ok(parse_pool_status(&payload)?)
    }

    pub async fn get_controller_config(&mut self) -> Result<ControllerConfig> {
        let (_, payload) = self.send_and_recv(build_get_controller_config()).await?;
        Ok(parse_controller_config(&payload)?)
    }

    pub async fn set_circuit(&mut self, circuit_id: i32, state: bool) -> Result<()> {
        let _ = self.send_and_recv(build_button_press(circuit_id, state)).await?;
        Ok(())
    }

    pub async fn set_heat_setpoint(&mut self, body_type: i32, temperature: i32) -> Result<()> {
        let _ = self
            .send_and_recv(build_set_heat_setpoint(body_type, temperature))
            .await?;
        Ok(())
    }

    pub async fn set_heat_mode(&mut self, body_type: i32, heat_mode: i32) -> Result<()> {
        let _ = self
            .send_and_recv(build_set_heat_mode(body_type, heat_mode))
            .await?;
        Ok(())
    }

    pub async fn set_cool_setpoint(&mut self, body_type: i32, temperature: i32) -> Result<()> {
        let _ = self
            .send_and_recv(build_set_cool_setpoint(body_type, temperature))
            .await?;

        // Verify the write took effect — some controllers silently ignore
        // cool setpoint on certain bodies (e.g., IntelliTouch ignores spa cool).
        let status = self.get_status().await?;
        if let Some(body) = status.bodies.iter().find(|b| b.body_type == body_type) {
            if body.cool_set_point != temperature {
                let body_name = if body_type == 0 { "pool" } else { "spa" };
                return Err(ClientError::WriteRejected(format!(
                    "{} cool setpoint not supported by this controller (sent {} but value is still {})",
                    body_name, temperature, body.cool_set_point
                )));
            }
        }
        Ok(())
    }

    pub async fn set_light_command(&mut self, command: i32) -> Result<()> {
        let _ = self
            .send_and_recv(build_color_lights_command(command))
            .await?;
        Ok(())
    }

    pub async fn get_chem_data(&mut self) -> Result<ChemData> {
        let (_, payload) = self.send_and_recv(build_get_chem_data()).await?;
        Ok(parse_chem_data(&payload)?)
    }

    pub async fn get_scg_config(&mut self) -> Result<ScgConfig> {
        let (_, payload) = self.send_and_recv(build_get_scg_config()).await?;
        Ok(parse_scg_config(&payload)?)
    }

    pub async fn set_scg_config(&mut self, pool_output: i32, spa_output: i32) -> Result<()> {
        let _ = self
            .send_and_recv(build_set_scg_config(pool_output, spa_output))
            .await?;
        Ok(())
    }

    pub async fn get_pump_status(&mut self, pump_index: i32) -> Result<PumpStatus> {
        let (_, payload) = self.send_and_recv(build_get_pump_status(pump_index)).await?;
        Ok(parse_pump_status(&payload)?)
    }

    pub async fn get_schedule_data(&mut self, schedule_type: i32) -> Result<ScheduleData> {
        let (_, payload) = self
            .send_and_recv(build_get_schedule_data(schedule_type))
            .await?;
        Ok(parse_schedule_data(&payload)?)
    }

    pub async fn add_schedule_event(&mut self, schedule_type: i32) -> Result<()> {
        let _ = self
            .send_and_recv(build_add_schedule_event(schedule_type))
            .await?;
        Ok(())
    }

    pub async fn delete_schedule_event(&mut self, schedule_id: i32) -> Result<()> {
        let _ = self
            .send_and_recv(build_delete_schedule_event(schedule_id))
            .await?;
        Ok(())
    }

    pub async fn set_schedule_event(
        &mut self,
        schedule_id: i32,
        circuit_id: i32,
        start_time: i32,
        stop_time: i32,
        day_mask: i32,
        heat_set_point: i32,
    ) -> Result<()> {
        let msg = build_set_schedule_event(
            schedule_id,
            circuit_id,
            start_time,
            stop_time,
            day_mask,
            heat_set_point,
        );
        let _ = self.send_and_recv(msg).await?;
        Ok(())
    }

    pub async fn get_weather(&mut self) -> Result<WeatherResponse> {
        let (_, payload) = self.send_and_recv(build_get_weather_forecast()).await?;
        Ok(parse_weather(&payload)?)
    }

    pub async fn get_system_time(&mut self) -> Result<SystemTimeResponse> {
        let (_, payload) = self.send_and_recv(build_get_system_time()).await?;
        Ok(parse_system_time(&payload)?)
    }

    pub async fn cancel_delay(&mut self) -> Result<()> {
        let _ = self.send_and_recv(build_cancel_delay()).await?;
        Ok(())
    }

    pub async fn ping(&mut self) -> Result<()> {
        let _ = self.send_and_recv(build_ping()).await?;
        Ok(())
    }

    /// Send a raw action with custom payload bytes. For the `raw` CLI command.
    pub async fn send_raw_action(
        &mut self,
        action: u16,
        payload: &[u8],
    ) -> Result<(MessageHeader, Vec<u8>)> {
        let msg = encode_message(action, payload);
        self.send_and_recv(msg).await
    }

    /// Graceful disconnect: remove client, then drop connection.
    pub async fn disconnect(mut self) -> Result<()> {
        let _ = self.send_raw(build_remove_client(self.client_id)).await;
        Ok(())
    }

    /// Get the client_id used for this session.
    pub fn client_id(&self) -> i32 {
        self.client_id
    }
}

fn is_push_message(action: u16) -> bool {
    matches!(action, 12500..=12505 | 9806)
}

fn rand_client_id() -> i32 {
    use std::time::SystemTime;
    let seed = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    // Range 32767-65535
    32767 + (seed as i32 % 32768).abs()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test that is_push_message correctly identifies push message action codes
    #[test]
    fn push_message_detection() {
        assert!(is_push_message(12500));
        assert!(is_push_message(12501));
        assert!(is_push_message(12502));
        assert!(is_push_message(12503));
        assert!(is_push_message(12504));
        assert!(is_push_message(12505));
        assert!(is_push_message(9806));
        assert!(!is_push_message(12527)); // StatusResponse - not push
        assert!(!is_push_message(8121)); // VersionResponse - not push
        assert!(!is_push_message(28)); // LoginResponse - not push
    }

    #[test]
    fn rand_client_id_in_range() {
        for _ in 0..100 {
            let id = rand_client_id();
            assert!(
                id >= 32767 && id <= 65535,
                "client_id {} out of range",
                id
            );
        }
    }
}
