use crate::config::SpaHeatNotificationsConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaHeatMilestone {
    HeatingStarted,
    EstimateReady,
    Halfway,
    AlmostReady,
    AtTemp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaHeatNotificationEvent {
    pub milestone: SpaHeatMilestone,
    pub current_temp: i32,
    pub target_temp: i32,
    pub minutes_remaining: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct SpaHeatNotificationState {
    run_active: bool,
    heating_started_sent: bool,
    estimate_ready_sent: bool,
    halfway_sent: bool,
    almost_ready_sent: bool,
    at_temp_sent: bool,
    trusted_session_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct SpaHeatNotificationInput {
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
    if !config.enabled || !input.heating_active {
        *state = SpaHeatNotificationState::default();
        return Vec::new();
    }

    let mut events = Vec::new();

    if !state.run_active {
        state.run_active = true;
        if config.heating_started {
            state.heating_started_sent = true;
            events.push(event(SpaHeatMilestone::HeatingStarted, input));
        }
    }

    if !events.is_empty() {
        return events;
    }

    if !state.estimate_ready_sent && input.minutes_remaining.is_some() && config.estimate_ready {
        state.estimate_ready_sent = true;
        events.push(event(SpaHeatMilestone::EstimateReady, input));
    }

    if !events.is_empty() {
        return events;
    }

    if let Some(session_id) = input.trusted_session_id {
        if state.trusted_session_id != Some(session_id) {
            state.trusted_session_id = Some(session_id);
            state.halfway_sent = false;
            state.almost_ready_sent = false;
            state.at_temp_sent = false;
        }
    }

    if let Some(progress) = trusted_progress(input) {
        let delta_f = (input.trusted_session_target_temp_f.unwrap_or_default()
            - input.trusted_session_start_temp_f.unwrap_or_default())
        .max(0.0);

        if delta_f >= config.minimum_delta_f {
            if config.halfway && !state.halfway_sent && progress >= 0.5 {
                state.halfway_sent = true;
                events.push(event(SpaHeatMilestone::Halfway, input));
            }
            if config.almost_ready && !state.almost_ready_sent && progress >= 0.9 {
                state.almost_ready_sent = true;
                events.push(event(SpaHeatMilestone::AlmostReady, input));
            }
        }
    }

    if config.at_temp && !state.at_temp_sent && input.current_temp >= input.target_temp {
        state.at_temp_sent = true;
        events.push(event(SpaHeatMilestone::AtTemp, input));
    }

    events
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
    SpaHeatNotificationEvent {
        milestone,
        current_temp: input.current_temp,
        target_temp: input.target_temp,
        minutes_remaining: input.minutes_remaining,
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
    fn notification_text_matches_expected_copy() {
        let (title, body) = notification_text(
            &SpaHeatNotificationEvent {
                milestone: SpaHeatMilestone::EstimateReady,
                current_temp: 92,
                target_temp: 104,
                minutes_remaining: Some(42),
            },
            "°F",
        );

        assert_eq!(title, "Spa ready in about 42 min");
        assert_eq!(body, "Current temperature 92°F");
    }
}
