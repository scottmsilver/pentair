use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    #[serde(default = "default_adapter_host")]
    pub adapter_host: String,
    #[serde(default = "default_bind")]
    pub bind: String,

    /// Override which circuits are spa accessories.
    /// By default, circuits named "jets", "blower", etc. are auto-detected.
    /// Use this to override if your circuit names don't match conventions.
    ///
    /// Example:
    /// ```toml
    /// [associations]
    /// spa = ["Bubbler", "Air Blower"]
    /// ```
    #[serde(default)]
    pub associations: Associations,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Associations {
    /// Circuit names to associate with the spa (as accessories like jets).
    #[serde(default)]
    pub spa: Vec<String>,
}

fn default_adapter_host() -> String { String::new() }
fn default_bind() -> String { "0.0.0.0:8080".to_string() }

impl Config {
    pub fn load(path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        if path.exists() {
            let contents = std::fs::read_to_string(path)?;
            Ok(toml::from_str(&contents)?)
        } else {
            Ok(Self::default())
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            adapter_host: default_adapter_host(),
            bind: default_bind(),
            associations: Default::default(),
        }
    }
}
