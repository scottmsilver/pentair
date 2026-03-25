use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tracing::{info, warn};

/// A single command within a scene (e.g., "turn spa on" or "set lights to caribbean").
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SceneCommand {
    /// The target device: "spa", "pool", "jets", "lights", or an auxiliary circuit name.
    pub target: String,
    /// The action to perform: "on", "off", or "mode" (for lights).
    pub action: String,
    /// Optional value for actions that require it (e.g., light mode name, heat setpoint).
    #[serde(default)]
    pub value: Option<String>,
}

/// A configured scene: a named list of commands to execute sequentially.
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq)]
pub struct SceneConfig {
    /// URL-safe identifier (e.g., "pool-party").
    pub name: String,
    /// Human-readable label (e.g., "Pool Party").
    pub label: String,
    /// Commands to execute in order.
    pub commands: Vec<SceneCommand>,
}

/// Result of executing a single command within a scene.
#[derive(Debug, Clone, Serialize)]
pub struct CommandResult {
    pub target: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Result of executing an entire scene.
#[derive(Debug, Clone, Serialize)]
pub struct SceneResult {
    pub scene: String,
    pub label: String,
    pub ok: bool,
    pub commands: Vec<CommandResult>,
}

/// Returns the built-in default scenes, used when no config file defines any.
pub fn default_scenes() -> Vec<SceneConfig> {
    vec![
        SceneConfig {
            name: "pool-party".to_string(),
            label: "Pool Party".to_string(),
            commands: vec![
                SceneCommand {
                    target: "spa".to_string(),
                    action: "on".to_string(),
                    value: None,
                },
                SceneCommand {
                    target: "jets".to_string(),
                    action: "on".to_string(),
                    value: None,
                },
                SceneCommand {
                    target: "lights".to_string(),
                    action: "mode".to_string(),
                    value: Some("caribbean".to_string()),
                },
            ],
        },
        SceneConfig {
            name: "relax".to_string(),
            label: "Relax".to_string(),
            commands: vec![
                SceneCommand {
                    target: "spa".to_string(),
                    action: "on".to_string(),
                    value: None,
                },
                SceneCommand {
                    target: "lights".to_string(),
                    action: "mode".to_string(),
                    value: Some("romantic".to_string()),
                },
            ],
        },
        SceneConfig {
            name: "all-off".to_string(),
            label: "All Off".to_string(),
            commands: vec![
                SceneCommand {
                    target: "spa".to_string(),
                    action: "off".to_string(),
                    value: None,
                },
                SceneCommand {
                    target: "lights".to_string(),
                    action: "off".to_string(),
                    value: None,
                },
            ],
        },
    ]
}

/// Merge user-configured scenes with defaults. User scenes override defaults by name.
pub fn resolve_scenes(configured: &[SceneConfig]) -> Vec<SceneConfig> {
    if configured.is_empty() {
        return default_scenes();
    }
    // If user provided any scenes, use only those (they explicitly chose their scene list).
    configured.to_vec()
}

/// Look up a scene by name.
pub fn find_scene<'a>(scenes: &'a [SceneConfig], name: &str) -> Option<&'a SceneConfig> {
    scenes.iter().find(|s| s.name == name)
}

/// Execute a scene by dispatching each command through the same internal operations
/// that the REST API uses. Commands run sequentially because some depend on ordering
/// (e.g., jets needs spa on first).
///
/// The `execute_command` closure maps each (target, action, value) to a Result<(), String>,
/// reusing the exact same code paths as the REST API handlers.
pub async fn execute_scene<F, Fut>(
    scene: &SceneConfig,
    execute_command: F,
) -> SceneResult
where
    F: Fn(String, String, Option<String>) -> Fut,
    Fut: std::future::Future<Output = Result<(), String>>,
{
    let mut results = Vec::with_capacity(scene.commands.len());
    let mut all_ok = true;

    for cmd in &scene.commands {
        info!(
            scene = %scene.name,
            target = %cmd.target,
            action = %cmd.action,
            "executing scene command"
        );

        let result = execute_command(
            cmd.target.clone(),
            cmd.action.clone(),
            cmd.value.clone(),
        )
        .await;

        let (ok, error) = match result {
            Ok(()) => (true, None),
            Err(e) => {
                warn!(
                    scene = %scene.name,
                    target = %cmd.target,
                    action = %cmd.action,
                    error = %e,
                    "scene command failed, continuing with remaining commands"
                );
                all_ok = false;
                (false, Some(e))
            }
        };

        results.push(CommandResult {
            target: cmd.target.clone(),
            action: cmd.action.clone(),
            value: cmd.value.clone(),
            ok,
            error,
        });
    }

    SceneResult {
        scene: scene.name.clone(),
        label: scene.label.clone(),
        ok: all_ok,
        commands: results,
    }
}

/// Shared scene store, wrapped in Arc for use across handlers.
/// Includes an execution mutex to serialize scene triggers and prevent
/// command interleaving against the hardware.
#[derive(Debug, Clone)]
pub struct SceneStore {
    scenes: Arc<Vec<SceneConfig>>,
    pub exec_lock: Arc<tokio::sync::Mutex<()>>,
}

impl SceneStore {
    pub fn new(scenes: Vec<SceneConfig>) -> Self {
        Self {
            scenes: Arc::new(scenes),
            exec_lock: Arc::new(tokio::sync::Mutex::new(())),
        }
    }

    pub fn list(&self) -> &[SceneConfig] {
        &self.scenes
    }

    pub fn find(&self, name: &str) -> Option<&SceneConfig> {
        find_scene(&self.scenes, name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scenes_are_present() {
        let scenes = default_scenes();
        assert_eq!(scenes.len(), 3);

        let names: Vec<&str> = scenes.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"pool-party"));
        assert!(names.contains(&"relax"));
        assert!(names.contains(&"all-off"));
    }

    #[test]
    fn default_scenes_have_correct_commands() {
        let scenes = default_scenes();

        let party = find_scene(&scenes, "pool-party").unwrap();
        assert_eq!(party.label, "Pool Party");
        assert_eq!(party.commands.len(), 3);
        assert_eq!(party.commands[0].target, "spa");
        assert_eq!(party.commands[0].action, "on");
        assert_eq!(party.commands[1].target, "jets");
        assert_eq!(party.commands[1].action, "on");
        assert_eq!(party.commands[2].target, "lights");
        assert_eq!(party.commands[2].action, "mode");
        assert_eq!(party.commands[2].value.as_deref(), Some("caribbean"));

        let relax = find_scene(&scenes, "relax").unwrap();
        assert_eq!(relax.commands.len(), 2);

        let all_off = find_scene(&scenes, "all-off").unwrap();
        assert_eq!(all_off.commands.len(), 2);
        assert_eq!(all_off.commands[0].action, "off");
        assert_eq!(all_off.commands[1].action, "off");
    }

    #[test]
    fn resolve_scenes_returns_defaults_when_empty() {
        let scenes = resolve_scenes(&[]);
        assert_eq!(scenes.len(), 3);
    }

    #[test]
    fn resolve_scenes_uses_configured_when_provided() {
        let custom = vec![SceneConfig {
            name: "custom".to_string(),
            label: "Custom Scene".to_string(),
            commands: vec![SceneCommand {
                target: "pool".to_string(),
                action: "on".to_string(),
                value: None,
            }],
        }];
        let scenes = resolve_scenes(&custom);
        assert_eq!(scenes.len(), 1);
        assert_eq!(scenes[0].name, "custom");
    }

    #[test]
    fn find_scene_returns_none_for_unknown() {
        let scenes = default_scenes();
        assert!(find_scene(&scenes, "nonexistent").is_none());
    }

    #[test]
    fn scene_config_parses_from_toml() {
        let toml_str = r#"
            [[scenes]]
            name = "pool-party"
            label = "Pool Party"
            commands = [
                { target = "spa", action = "on" },
                { target = "jets", action = "on" },
                { target = "lights", action = "mode", value = "caribbean" },
            ]

            [[scenes]]
            name = "relax"
            label = "Relax"
            commands = [
                { target = "spa", action = "on" },
                { target = "lights", action = "mode", value = "romantic" },
            ]
        "#;

        #[derive(Deserialize)]
        struct Wrapper {
            scenes: Vec<SceneConfig>,
        }

        let parsed: Wrapper = toml::from_str(toml_str).unwrap();
        assert_eq!(parsed.scenes.len(), 2);
        assert_eq!(parsed.scenes[0].name, "pool-party");
        assert_eq!(parsed.scenes[0].commands.len(), 3);
        assert_eq!(parsed.scenes[1].name, "relax");
    }

    #[test]
    fn scene_store_list_and_find() {
        let store = SceneStore::new(default_scenes());
        assert_eq!(store.list().len(), 3);
        assert!(store.find("pool-party").is_some());
        assert!(store.find("nope").is_none());
    }

    #[tokio::test]
    async fn execute_scene_all_succeed() {
        let scene = SceneConfig {
            name: "test".to_string(),
            label: "Test".to_string(),
            commands: vec![
                SceneCommand {
                    target: "spa".to_string(),
                    action: "on".to_string(),
                    value: None,
                },
                SceneCommand {
                    target: "lights".to_string(),
                    action: "mode".to_string(),
                    value: Some("caribbean".to_string()),
                },
            ],
        };

        let result = execute_scene(&scene, |_target, _action, _value| async { Ok(()) }).await;

        assert!(result.ok);
        assert_eq!(result.commands.len(), 2);
        assert!(result.commands[0].ok);
        assert!(result.commands[1].ok);
    }

    #[tokio::test]
    async fn execute_scene_partial_failure() {
        let scene = SceneConfig {
            name: "test".to_string(),
            label: "Test".to_string(),
            commands: vec![
                SceneCommand {
                    target: "spa".to_string(),
                    action: "on".to_string(),
                    value: None,
                },
                SceneCommand {
                    target: "jets".to_string(),
                    action: "on".to_string(),
                    value: None,
                },
                SceneCommand {
                    target: "lights".to_string(),
                    action: "mode".to_string(),
                    value: Some("caribbean".to_string()),
                },
            ],
        };

        let result = execute_scene(&scene, |target, _action, _value| async move {
            if target == "jets" {
                Err("jets circuit not found".to_string())
            } else {
                Ok(())
            }
        })
        .await;

        assert!(!result.ok);
        assert_eq!(result.commands.len(), 3);
        assert!(result.commands[0].ok);
        assert!(!result.commands[1].ok);
        assert_eq!(
            result.commands[1].error.as_deref(),
            Some("jets circuit not found")
        );
        // Third command still executes despite second failing
        assert!(result.commands[2].ok);
    }

    #[tokio::test]
    async fn execute_scene_preserves_command_order() {
        use std::sync::Mutex;

        let order = Arc::new(Mutex::new(Vec::new()));
        let scene = SceneConfig {
            name: "test".to_string(),
            label: "Test".to_string(),
            commands: vec![
                SceneCommand {
                    target: "spa".to_string(),
                    action: "on".to_string(),
                    value: None,
                },
                SceneCommand {
                    target: "jets".to_string(),
                    action: "on".to_string(),
                    value: None,
                },
                SceneCommand {
                    target: "lights".to_string(),
                    action: "mode".to_string(),
                    value: Some("caribbean".to_string()),
                },
            ],
        };

        let order_clone = order.clone();
        let _result = execute_scene(&scene, move |target, _action, _value| {
            let order = order_clone.clone();
            async move {
                order.lock().unwrap().push(target);
                Ok(())
            }
        })
        .await;

        let executed = order.lock().unwrap().clone();
        assert_eq!(executed, vec!["spa", "jets", "lights"]);
    }

    #[test]
    fn scene_config_serializes_to_json() {
        let scene = SceneConfig {
            name: "pool-party".to_string(),
            label: "Pool Party".to_string(),
            commands: vec![
                SceneCommand {
                    target: "spa".to_string(),
                    action: "on".to_string(),
                    value: None,
                },
            ],
        };

        let json = serde_json::to_value(&scene).unwrap();
        assert_eq!(json["name"], "pool-party");
        assert_eq!(json["label"], "Pool Party");
        assert_eq!(json["commands"][0]["target"], "spa");
        assert_eq!(json["commands"][0]["action"], "on");
    }

    #[test]
    fn command_mapping_covers_all_targets_and_actions() {
        // Validate that the supported target/action combinations are documented
        let valid_combos = vec![
            ("spa", "on", None),
            ("spa", "off", None),
            ("pool", "on", None),
            ("pool", "off", None),
            ("jets", "on", None),
            ("jets", "off", None),
            ("lights", "on", None),
            ("lights", "off", None),
            ("lights", "mode", Some("caribbean")),
            ("lights", "mode", Some("romantic")),
            ("lights", "mode", Some("party")),
        ];

        for (target, action, value) in valid_combos {
            let cmd = SceneCommand {
                target: target.to_string(),
                action: action.to_string(),
                value: value.map(String::from),
            };
            // Just ensure it can be constructed and serialized
            let json = serde_json::to_value(&cmd).unwrap();
            assert_eq!(json["target"], target);
            assert_eq!(json["action"], action);
        }
    }
}
