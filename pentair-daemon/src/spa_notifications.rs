use crate::config::SpaHeatNotificationsConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaHeatMilestone {
    HeatingStarted,
    EstimateReady,
    Halfway,
    AlmostReady,
    AtTemp,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SpaHeatNotificationEvent {
    pub milestone: SpaHeatMilestone,
    pub current_temp: i32,
    pub target_temp: i32,
    pub minutes_remaining: Option<u32>,
    pub start_temp_f: Option<f64>,
    pub progress_pct: Option<u8>,
    pub session_id: String,
}

#[derive(Debug, Clone)]
pub enum SpaHeatNotificationState {
    Idle,
    Heating {
        heating_started_sent: bool,
        estimate_ready_sent: bool,
        halfway_sent: bool,
        almost_ready_sent: bool,
        at_temp_sent: bool,
        trusted_session_id: Option<i64>,
    },
    Maintaining {
        setpoint: i32,
    },
}

impl Default for SpaHeatNotificationState {
    fn default() -> Self {
        Self::Idle
    }
}

#[derive(Debug, Clone)]
pub struct SpaHeatNotificationInput {
    pub spa_on: bool,
    pub heat_mode_off: bool,
    pub heating_active: bool,
    pub current_temp: i32,
    pub target_temp: i32,
    pub minutes_remaining: Option<u32>,
    pub trusted_session_start_temp_f: Option<f64>,
    pub trusted_session_current_temp_f: Option<f64>,
    pub trusted_session_target_temp_f: Option<f64>,
    pub trusted_session_id: Option<i64>,
}

pub fn evaluate_spa_heat_notifications(
    config: &SpaHeatNotificationsConfig,
    input: &SpaHeatNotificationInput,
    state: &mut SpaHeatNotificationState,
) -> Vec<SpaHeatNotificationEvent> {
    if !config.enabled || !input.spa_on || input.heat_mode_off {
        *state = SpaHeatNotificationState::Idle;
        return Vec::new();
    }

    match state {
        SpaHeatNotificationState::Idle => {
            if input.heating_active {
                let mut heating_started_sent = false;
                let mut events = Vec::new();
                if config.heating_started {
                    heating_started_sent = true;
                    events.push(event(SpaHeatMilestone::HeatingStarted, input));
                }
                *state = SpaHeatNotificationState::Heating {
                    heating_started_sent,
                    estimate_ready_sent: false,
                    halfway_sent: false,
                    almost_ready_sent: false,
                    at_temp_sent: false,
                    trusted_session_id: None,
                };
                events
            } else {
                Vec::new()
            }
        }

        SpaHeatNotificationState::Heating {
            heating_started_sent,
            estimate_ready_sent,
            halfway_sent,
            almost_ready_sent,
            at_temp_sent,
            trusted_session_id,
        } => {
            if !input.heating_active {
                // Heater cycled off but spa still on — don't change phase,
                // just skip this cycle. Milestones resume when heater kicks back on.
                return Vec::new();
            }

            let mut events = Vec::new();

            if !*heating_started_sent && config.heating_started {
                *heating_started_sent = true;
                events.push(event(SpaHeatMilestone::HeatingStarted, input));
                return events;
            }

            if !*estimate_ready_sent && input.minutes_remaining.is_some() && config.estimate_ready {
                *estimate_ready_sent = true;
                events.push(event(SpaHeatMilestone::EstimateReady, input));
                return events;
            }

            if let Some(session_id) = input.trusted_session_id {
                if *trusted_session_id != Some(session_id) {
                    *trusted_session_id = Some(session_id);
                    *halfway_sent = false;
                    *almost_ready_sent = false;
                    *at_temp_sent = false;
                }
            }

            if let Some(progress) = trusted_progress(input) {
                let delta_f = (input.trusted_session_target_temp_f.unwrap_or_default()
                    - input.trusted_session_start_temp_f.unwrap_or_default())
                .max(0.0);

                if delta_f >= config.minimum_delta_f {
                    if config.halfway && !*halfway_sent && progress >= 0.5 {
                        *halfway_sent = true;
                        events.push(event(SpaHeatMilestone::Halfway, input));
                    }
                    if config.almost_ready && !*almost_ready_sent && progress >= 0.9 {
                        *almost_ready_sent = true;
                        events.push(event(SpaHeatMilestone::AlmostReady, input));
                    }
                }
            }

            if config.at_temp && !*at_temp_sent && input.current_temp >= input.target_temp {
                *at_temp_sent = true;
                events.push(event(SpaHeatMilestone::AtTemp, input));
                *state = SpaHeatNotificationState::Maintaining {
                    setpoint: input.target_temp,
                };
            }

            events
        }

        SpaHeatNotificationState::Maintaining { setpoint } => {
            let prev_setpoint = *setpoint;
            // Always track current setpoint so future raises are detected
            // relative to the latest value, not the historical max.
            *setpoint = input.target_temp;

            if input.target_temp > prev_setpoint {
                // Setpoint raised — new heating intent
                let mut heating_started_sent = false;
                let mut events = Vec::new();
                if config.heating_started {
                    heating_started_sent = true;
                    events.push(event(SpaHeatMilestone::HeatingStarted, input));
                }
                *state = SpaHeatNotificationState::Heating {
                    heating_started_sent,
                    estimate_ready_sent: false,
                    halfway_sent: false,
                    almost_ready_sent: false,
                    at_temp_sent: false,
                    trusted_session_id: None,
                };
                events
            } else {
                // Heater cycling or setpoint lowered — silent
                Vec::new()
            }
        }
    }
}

pub fn notification_text(event: &SpaHeatNotificationEvent, temp_unit: &str) -> (String, String) {
    match event.milestone {
        SpaHeatMilestone::HeatingStarted => (
            "Spa heating started".to_string(),
            format!("Heating to {}{}", event.target_temp, temp_unit),
        ),
        SpaHeatMilestone::EstimateReady => match event.minutes_remaining {
            Some(minutes) => (
                format!("Spa ready in about {} min", minutes),
                format!("Current temperature {}{}", event.current_temp, temp_unit),
            ),
            None => (
                "Spa is heating — estimate calculating...".to_string(),
                format!("Current temperature {}{}", event.current_temp, temp_unit),
            ),
        },
        SpaHeatMilestone::Halfway => (
            "Spa warming up".to_string(),
            format!("About halfway to {}{}", event.target_temp, temp_unit),
        ),
        SpaHeatMilestone::AlmostReady => (
            "Spa almost ready".to_string(),
            format!("About 10% left to {}{}", event.target_temp, temp_unit),
        ),
        SpaHeatMilestone::AtTemp => (
            "Spa ready".to_string(),
            format!("Spa has reached {}{}", event.target_temp, temp_unit),
        ),
    }
}

fn trusted_progress(input: &SpaHeatNotificationInput) -> Option<f64> {
    let start = input.trusted_session_start_temp_f?;
    let current = input.trusted_session_current_temp_f?;
    let target = input.trusted_session_target_temp_f?;
    let delta = target - start;
    if !delta.is_finite() || delta <= 0.0 {
        return None;
    }
    Some(((current - start) / delta).clamp(0.0, 1.0))
}

fn event(
    milestone: SpaHeatMilestone,
    input: &SpaHeatNotificationInput,
) -> SpaHeatNotificationEvent {
    let progress_pct = trusted_progress(input).map(|p| (p * 100.0).round() as u8);
    let session_id = input
        .trusted_session_id
        .map(|id| {
            chrono::DateTime::from_timestamp(id, 0)
                .map(|dt| dt.to_rfc3339())
                .unwrap_or_else(|| id.to_string())
        })
        .unwrap_or_default();
    SpaHeatNotificationEvent {
        milestone,
        current_temp: input.current_temp,
        target_temp: input.target_temp,
        minutes_remaining: input.minutes_remaining,
        start_temp_f: input.trusted_session_start_temp_f,
        progress_pct,
        session_id,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> SpaHeatNotificationsConfig {
        SpaHeatNotificationsConfig {
            enabled: true,
            heating_started: true,
            estimate_ready: true,
            halfway: true,
            almost_ready: true,
            at_temp: true,
            minimum_delta_f: 4.0,
        }
    }

    fn base_input() -> SpaHeatNotificationInput {
        SpaHeatNotificationInput {
            spa_on: true,
            heat_mode_off: false,
            heating_active: true,
            current_temp: 92,
            target_temp: 104,
            minutes_remaining: None,
            trusted_session_start_temp_f: None,
            trusted_session_current_temp_f: None,
            trusted_session_target_temp_f: None,
            trusted_session_id: None,
        }
    }

    #[test]
    fn heating_started_fires_once_when_heating_begins() {
        let config = base_config();
        let mut state = SpaHeatNotificationState::default();

        let first = evaluate_spa_heat_notifications(&config, &base_input(), &mut state);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].milestone, SpaHeatMilestone::HeatingStarted);

        let second = evaluate_spa_heat_notifications(&config, &base_input(), &mut state);
        assert!(second.is_empty());
    }

    #[test]
    fn estimate_ready_waits_for_first_eta_and_fires_once() {
        let config = base_config();
        let mut state = SpaHeatNotificationState::default();
        let mut input = base_input();

        let started = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert_eq!(started.len(), 1);

        let before_eta = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(before_eta.is_empty());

        input.minutes_remaining = Some(42);
        let ready = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].milestone, SpaHeatMilestone::EstimateReady);

        let repeated = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(repeated.is_empty());
    }

    #[test]
    fn estimate_ready_suppresses_progress_on_same_update() {
        let config = base_config();
        let mut state = SpaHeatNotificationState::default();
        let mut input = base_input();

        evaluate_spa_heat_notifications(&config, &input, &mut state);

        input.minutes_remaining = Some(20);
        input.trusted_session_id = Some(1);
        input.trusted_session_start_temp_f = Some(92.0);
        input.trusted_session_current_temp_f = Some(100.0);
        input.trusted_session_target_temp_f = Some(104.0);

        let ready = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].milestone, SpaHeatMilestone::EstimateReady);

        let progress = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(progress.iter().any(|event| event.milestone == SpaHeatMilestone::Halfway));
    }

    #[test]
    fn progress_milestones_fire_once_for_trusted_session() {
        let config = base_config();
        let mut state = SpaHeatNotificationState::default();
        let mut input = base_input();

        evaluate_spa_heat_notifications(&config, &input, &mut state);

        input.minutes_remaining = Some(42);
        input.trusted_session_id = Some(1);
        input.trusted_session_start_temp_f = Some(92.0);
        input.trusted_session_current_temp_f = Some(98.0);
        input.trusted_session_target_temp_f = Some(104.0);
        let ready = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(ready.iter().any(|event| event.milestone == SpaHeatMilestone::EstimateReady));

        let halfway = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(halfway.iter().any(|event| event.milestone == SpaHeatMilestone::Halfway));

        input.trusted_session_current_temp_f = Some(103.0);
        let almost_ready = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(almost_ready.iter().any(|event| event.milestone == SpaHeatMilestone::AlmostReady));

        input.current_temp = 104;
        input.trusted_session_current_temp_f = Some(104.0);
        input.minutes_remaining = Some(1);
        let at_temp = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(at_temp.iter().any(|event| event.milestone == SpaHeatMilestone::AtTemp));

        // After AtTemp, state should be Maintaining — reheat cycles are silent
        let repeated = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(repeated.is_empty());
    }

    #[test]
    fn small_delta_skips_progress_but_allows_at_temp() {
        let config = base_config();
        let mut state = SpaHeatNotificationState::default();
        let mut input = base_input();

        evaluate_spa_heat_notifications(&config, &input, &mut state);

        input.minutes_remaining = Some(8);
        input.current_temp = 101;
        input.target_temp = 104;
        input.trusted_session_id = Some(2);
        input.trusted_session_start_temp_f = Some(101.0);
        input.trusted_session_current_temp_f = Some(103.0);
        input.trusted_session_target_temp_f = Some(104.0);

        let progress = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(!progress.iter().any(|event| event.milestone == SpaHeatMilestone::Halfway));
        assert!(!progress.iter().any(|event| event.milestone == SpaHeatMilestone::AlmostReady));

        input.current_temp = 104;
        input.trusted_session_current_temp_f = Some(104.0);
        let at_temp = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(at_temp.iter().any(|event| event.milestone == SpaHeatMilestone::AtTemp));
    }

    #[test]
    fn reheat_cycle_is_silent_in_maintaining() {
        let config = base_config();
        let mut state = SpaHeatNotificationState::default();
        let mut input = base_input();

        // Heat up to AtTemp
        evaluate_spa_heat_notifications(&config, &input, &mut state); // HeatingStarted
        input.current_temp = 104;
        let at_temp = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(at_temp.iter().any(|e| e.milestone == SpaHeatMilestone::AtTemp));
        assert!(matches!(state, SpaHeatNotificationState::Maintaining { .. }));

        // Heater cycles off (spa still on)
        input.heating_active = false;
        input.current_temp = 102;
        let silent = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(silent.is_empty());
        assert!(matches!(state, SpaHeatNotificationState::Maintaining { .. }));

        // Heater kicks back on — still silent
        input.heating_active = true;
        let still_silent = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(still_silent.is_empty());
        assert!(matches!(state, SpaHeatNotificationState::Maintaining { .. }));
    }

    #[test]
    fn setpoint_raise_in_maintaining_starts_new_heating() {
        let config = base_config();
        let mut state = SpaHeatNotificationState::default();
        let mut input = base_input();
        input.target_temp = 102;

        // Heat up to AtTemp at 102
        evaluate_spa_heat_notifications(&config, &input, &mut state);
        input.current_temp = 102;
        evaluate_spa_heat_notifications(&config, &input, &mut state); // AtTemp
        assert!(matches!(state, SpaHeatNotificationState::Maintaining { setpoint: 102 }));

        // Raise setpoint to 106
        input.target_temp = 106;
        input.current_temp = 102;
        input.heating_active = true;
        let events = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].milestone, SpaHeatMilestone::HeatingStarted);
        assert!(matches!(state, SpaHeatNotificationState::Heating { .. }));
    }

    #[test]
    fn setpoint_lower_in_maintaining_stays_silent() {
        let config = base_config();
        let mut state = SpaHeatNotificationState::default();
        let mut input = base_input();

        // Heat up to AtTemp at 104
        evaluate_spa_heat_notifications(&config, &input, &mut state);
        input.current_temp = 104;
        evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(matches!(state, SpaHeatNotificationState::Maintaining { setpoint: 104 }));

        // Lower setpoint to 100
        input.target_temp = 100;
        let events = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(events.is_empty());
        assert!(matches!(state, SpaHeatNotificationState::Maintaining { .. }));
    }

    #[test]
    fn spa_off_resets_to_idle_then_fresh_heating() {
        let config = base_config();
        let mut state = SpaHeatNotificationState::default();
        let mut input = base_input();

        // Heat up to Maintaining
        evaluate_spa_heat_notifications(&config, &input, &mut state);
        input.current_temp = 104;
        evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(matches!(state, SpaHeatNotificationState::Maintaining { .. }));

        // Spa turned off
        input.spa_on = false;
        input.heating_active = false;
        let events = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(events.is_empty());
        assert!(matches!(state, SpaHeatNotificationState::Idle));

        // Spa turned back on — fresh HeatingStarted
        input.spa_on = true;
        input.heating_active = true;
        input.current_temp = 92;
        let events = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].milestone, SpaHeatMilestone::HeatingStarted);
    }

    #[test]
    fn heater_off_during_heating_pauses_milestones() {
        let config = base_config();
        let mut state = SpaHeatNotificationState::default();
        let mut input = base_input();

        // Start heating
        evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(matches!(state, SpaHeatNotificationState::Heating { .. }));

        // Heater cycles off briefly (spa still on)
        input.heating_active = false;
        let events = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(events.is_empty());
        // Should stay in Heating, not reset
        assert!(matches!(state, SpaHeatNotificationState::Heating { .. }));

        // Heater kicks back on — no duplicate HeatingStarted
        input.heating_active = true;
        let events = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(!events.iter().any(|e| e.milestone == SpaHeatMilestone::HeatingStarted));
    }

    #[test]
    fn setpoint_lowered_during_heating_adjusts_goal() {
        let config = base_config();
        let mut state = SpaHeatNotificationState::default();
        let mut input = base_input();
        // Start at 80, target 104
        input.current_temp = 80;
        input.target_temp = 104;

        // HeatingStarted fires
        let events = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert_eq!(events[0].milestone, SpaHeatMilestone::HeatingStarted);

        // Heating progresses to 90
        input.current_temp = 90;
        input.minutes_remaining = Some(30);
        input.trusted_session_id = Some(1);
        input.trusted_session_start_temp_f = Some(80.0);
        input.trusted_session_current_temp_f = Some(90.0);
        input.trusted_session_target_temp_f = Some(104.0);
        evaluate_spa_heat_notifications(&config, &input, &mut state); // EstimateReady

        // User lowers setpoint to 102 — new session from heat estimator
        input.target_temp = 102;
        input.trusted_session_id = Some(2);
        input.trusted_session_target_temp_f = Some(102.0);
        input.trusted_session_start_temp_f = Some(80.0);
        input.trusted_session_current_temp_f = Some(90.0);

        // Should NOT fire HeatingStarted again — stays in Heating
        let events = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(!events.iter().any(|e| e.milestone == SpaHeatMilestone::HeatingStarted));
        assert!(matches!(state, SpaHeatNotificationState::Heating { .. }));

        // AtTemp fires at 102, not 104
        input.current_temp = 102;
        input.trusted_session_current_temp_f = Some(102.0);
        let events = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(events.iter().any(|e| e.milestone == SpaHeatMilestone::AtTemp));
        assert_eq!(events.iter().find(|e| e.milestone == SpaHeatMilestone::AtTemp).unwrap().target_temp, 102);
        assert!(matches!(state, SpaHeatNotificationState::Maintaining { setpoint: 102 }));
    }

    #[test]
    fn setpoint_lowered_below_current_temp_during_heating_fires_at_temp() {
        let config = base_config();
        let mut state = SpaHeatNotificationState::default();
        let mut input = base_input();
        input.current_temp = 95;
        input.target_temp = 104;

        // HeatingStarted
        evaluate_spa_heat_notifications(&config, &input, &mut state);

        // User lowers setpoint to 90 — already past the new goal
        input.target_temp = 90;
        let events = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(events.iter().any(|e| e.milestone == SpaHeatMilestone::AtTemp));
        assert!(matches!(state, SpaHeatNotificationState::Maintaining { setpoint: 90 }));
    }

    #[test]
    fn heat_mode_off_resets_to_idle() {
        let config = base_config();
        let mut state = SpaHeatNotificationState::default();
        let mut input = base_input();

        // Heat up to Maintaining
        evaluate_spa_heat_notifications(&config, &input, &mut state);
        input.current_temp = 104;
        evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(matches!(state, SpaHeatNotificationState::Maintaining { .. }));

        // User turns heat mode off (spa circuit still on)
        input.heat_mode_off = true;
        input.heating_active = false;
        let events = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(events.is_empty());
        assert!(matches!(state, SpaHeatNotificationState::Idle));

        // User turns heat mode back on — fresh HeatingStarted
        input.heat_mode_off = false;
        input.heating_active = true;
        input.current_temp = 100;
        let events = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].milestone, SpaHeatMilestone::HeatingStarted);
    }

    #[test]
    fn setpoint_lower_then_raise_in_maintaining_triggers_heating() {
        let config = base_config();
        let mut state = SpaHeatNotificationState::default();
        let mut input = base_input();

        // Heat up to AtTemp at 104
        evaluate_spa_heat_notifications(&config, &input, &mut state);
        input.current_temp = 104;
        evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(matches!(state, SpaHeatNotificationState::Maintaining { setpoint: 104 }));

        // Lower setpoint to 100 — silent, but stored setpoint updates
        input.target_temp = 100;
        let events = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert!(events.is_empty());
        assert!(matches!(state, SpaHeatNotificationState::Maintaining { setpoint: 100 }));

        // Raise setpoint to 102 — this is above 100, so new heating intent
        input.target_temp = 102;
        input.heating_active = true;
        let events = evaluate_spa_heat_notifications(&config, &input, &mut state);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].milestone, SpaHeatMilestone::HeatingStarted);
        assert!(matches!(state, SpaHeatNotificationState::Heating { .. }));
    }

    #[test]
    fn notification_text_matches_expected_copy() {
        let (title, body) = notification_text(
            &SpaHeatNotificationEvent {
                milestone: SpaHeatMilestone::EstimateReady,
                current_temp: 92,
                target_temp: 104,
                minutes_remaining: Some(42),
                start_temp_f: Some(80.0),
                progress_pct: Some(50),
                session_id: String::new(),
            },
            "°F",
        );

        assert_eq!(title, "Spa ready in about 42 min");
        assert_eq!(body, "Current temperature 92°F");
    }
}
