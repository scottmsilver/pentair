use pentair_client::client::Client;
use pentair_protocol::codec::MessageHeader;
use pentair_protocol::responses::*;

pub enum Backend {
    Direct(Client),
    Daemon(DaemonClient),
}

pub struct DaemonClient {
    base_url: String,
    http: reqwest::Client,
}

impl DaemonClient {
    pub fn new(base_url: String) -> Self {
        Self {
            base_url,
            http: reqwest::Client::new(),
        }
    }

    /// GET a JSON value from the daemon, returning an error if the response is null
    /// (meaning the daemon hasn't cached the data yet).
    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, Box<dyn std::error::Error>> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self.http.get(&url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("daemon returned {}: {}", status, body).into());
        }
        let value: serde_json::Value = resp.json().await?;
        if value.is_null() {
            return Err(format!(
                "daemon has no cached data for {} yet (try again shortly, or use --direct)",
                path
            )
            .into());
        }
        Ok(serde_json::from_value(value)?)
    }

    /// POST JSON to the daemon and check for {"ok": true}.
    async fn post_json(
        &self,
        path: &str,
        body: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self.http.post(&url).json(body).send().await?;
        let status = resp.status();
        let result: serde_json::Value = resp.json().await?;
        if !status.is_success() {
            let err = result
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown error");
            return Err(format!("daemon error: {}", err).into());
        }
        if let Some(ok) = result.get("ok") {
            if ok.as_bool() == Some(false) {
                let err = result
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("unknown error");
                return Err(format!("daemon error: {}", err).into());
            }
        }
        Ok(())
    }

    /// POST to the daemon with no request body.
    async fn post_empty(&self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let url = format!("{}{}", self.base_url, path);
        let resp = self.http.post(&url).send().await?;
        let status = resp.status();
        let result: serde_json::Value = resp.json().await?;
        if !status.is_success() {
            let err = result
                .get("error")
                .and_then(|e| e.as_str())
                .unwrap_or("unknown error");
            return Err(format!("daemon error: {}", err).into());
        }
        if let Some(ok) = result.get("ok") {
            if ok.as_bool() == Some(false) {
                let err = result
                    .get("error")
                    .and_then(|e| e.as_str())
                    .unwrap_or("unknown error");
                return Err(format!("daemon error: {}", err).into());
            }
        }
        Ok(())
    }
}

impl Backend {
    pub fn client_id(&self) -> Result<i32, Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.client_id()),
            Backend::Daemon(_) => {
                Err("client id requires --direct mode (no daemon endpoint)".into())
            }
        }
    }

    pub async fn get_version(&mut self) -> Result<VersionResponse, Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.get_version().await?),
            Backend::Daemon(d) => d.get_json("/api/version").await,
        }
    }

    pub async fn get_status(&mut self) -> Result<PoolStatus, Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.get_status().await?),
            Backend::Daemon(d) => d.get_json("/api/status").await,
        }
    }

    pub async fn get_system_time(
        &mut self,
    ) -> Result<SystemTimeResponse, Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.get_system_time().await?),
            Backend::Daemon(_) => {
                Err("system time command requires --direct mode (no daemon endpoint)".into())
            }
        }
    }

    pub async fn get_controller_config(
        &mut self,
    ) -> Result<ControllerConfig, Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.get_controller_config().await?),
            Backend::Daemon(d) => d.get_json("/api/config").await,
        }
    }

    pub async fn set_circuit(
        &mut self,
        circuit_id: i32,
        state: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.set_circuit(circuit_id, state).await?),
            Backend::Daemon(d) => {
                d.post_json(
                    &format!("/api/circuits/{}", circuit_id),
                    &serde_json::json!({ "state": state }),
                )
                .await
            }
        }
    }

    pub async fn set_heat_setpoint(
        &mut self,
        body_type: i32,
        temp: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.set_heat_setpoint(body_type, temp).await?),
            Backend::Daemon(d) => {
                d.post_json(
                    "/api/heat/setpoint",
                    &serde_json::json!({ "body_type": body_type, "temperature": temp }),
                )
                .await
            }
        }
    }

    pub async fn set_heat_mode(
        &mut self,
        body_type: i32,
        mode: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.set_heat_mode(body_type, mode).await?),
            Backend::Daemon(d) => {
                d.post_json(
                    "/api/heat/mode",
                    &serde_json::json!({ "body_type": body_type, "mode": mode }),
                )
                .await
            }
        }
    }

    pub async fn set_cool_setpoint(
        &mut self,
        body_type: i32,
        temp: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.set_cool_setpoint(body_type, temp).await?),
            Backend::Daemon(d) => {
                d.post_json(
                    "/api/heat/cool",
                    &serde_json::json!({ "body_type": body_type, "temperature": temp }),
                )
                .await
            }
        }
    }

    pub async fn set_light_command(
        &mut self,
        command: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.set_light_command(command).await?),
            Backend::Daemon(d) => {
                d.post_json("/api/lights", &serde_json::json!({ "command": command }))
                    .await
            }
        }
    }

    pub async fn get_chem_data(&mut self) -> Result<ChemData, Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.get_chem_data().await?),
            Backend::Daemon(d) => d.get_json("/api/chem").await,
        }
    }

    pub async fn get_scg_config(&mut self) -> Result<ScgConfig, Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.get_scg_config().await?),
            Backend::Daemon(d) => d.get_json("/api/chlor").await,
        }
    }

    pub async fn set_scg_config(
        &mut self,
        pool: i32,
        spa: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.set_scg_config(pool, spa).await?),
            Backend::Daemon(d) => {
                d.post_json(
                    "/api/chlor/set",
                    &serde_json::json!({ "pool": pool, "spa": spa }),
                )
                .await
            }
        }
    }

    pub async fn get_pump_status(
        &mut self,
        index: i32,
    ) -> Result<PumpStatus, Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.get_pump_status(index).await?),
            Backend::Daemon(d) => d.get_json(&format!("/api/pumps/{}", index)).await,
        }
    }

    pub async fn get_schedule_data(
        &mut self,
        schedule_type: i32,
    ) -> Result<ScheduleData, Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.get_schedule_data(schedule_type).await?),
            Backend::Daemon(_) => {
                Err("schedule commands require --direct mode (no daemon endpoint)".into())
            }
        }
    }

    pub async fn get_history(
        &mut self,
        start_time: &pentair_protocol::types::SLDateTime,
        end_time: &pentair_protocol::types::SLDateTime,
        sender_id: i32,
    ) -> Result<HistoryData, Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => {
                Ok(client.get_history(start_time, end_time, sender_id).await?)
            }
            Backend::Daemon(_) => {
                Err("history command requires --direct mode (no daemon endpoint)".into())
            }
        }
    }

    pub async fn add_schedule_event(
        &mut self,
        schedule_type: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.add_schedule_event(schedule_type).await?),
            Backend::Daemon(_) => {
                Err("schedule commands require --direct mode (no daemon endpoint)".into())
            }
        }
    }

    pub async fn delete_schedule_event(
        &mut self,
        id: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.delete_schedule_event(id).await?),
            Backend::Daemon(_) => {
                Err("schedule commands require --direct mode (no daemon endpoint)".into())
            }
        }
    }

    pub async fn set_schedule_event(
        &mut self,
        id: i32,
        circuit_id: i32,
        start: i32,
        stop: i32,
        day_mask: i32,
        heat: i32,
    ) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client
                .set_schedule_event(id, circuit_id, start, stop, day_mask, heat)
                .await?),
            Backend::Daemon(_) => {
                Err("schedule commands require --direct mode (no daemon endpoint)".into())
            }
        }
    }

    pub async fn get_weather(&mut self) -> Result<WeatherResponse, Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.get_weather().await?),
            Backend::Daemon(_) => {
                Err("weather command requires --direct mode (no daemon endpoint)".into())
            }
        }
    }

    pub async fn cancel_delay(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.cancel_delay().await?),
            Backend::Daemon(d) => d.post_empty("/api/cancel-delay").await,
        }
    }

    pub async fn send_raw_action(
        &mut self,
        action: u16,
        payload: &[u8],
    ) -> Result<(MessageHeader, Vec<u8>), Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.send_raw_action(action, payload).await?),
            Backend::Daemon(_) => {
                Err("raw command requires --direct mode (no daemon endpoint)".into())
            }
        }
    }

    pub async fn disconnect(self) -> Result<(), Box<dyn std::error::Error>> {
        match self {
            Backend::Direct(client) => Ok(client.disconnect().await?),
            Backend::Daemon(_) => Ok(()), // no-op
        }
    }
}
