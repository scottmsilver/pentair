use serde::Deserialize;
use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct Config {
    pub daemon_url: String,
    pub discriminator: u16,
    pub fabric_path: PathBuf,
}

#[derive(Debug, Deserialize, Default)]
struct FileConfig {
    daemon_url: Option<String>,
    discriminator: Option<u16>,
    fabric_path: Option<String>,
}

impl Config {
    /// Load config: CLI args override TOML file override defaults.
    /// Note: passcode is not configurable — rs-matter's Spake2pVerifierPassword
    /// is pub(crate), so we always use the test default (20202021).
    pub fn load(
        daemon_url: Option<String>,
        discriminator: Option<u16>,
        config_path: Option<PathBuf>,
    ) -> Self {
        let file_config = config_path
            .or_else(|| {
                dirs::home_dir().map(|h| h.join(".pentair").join("matter.toml"))
            })
            .and_then(|p| std::fs::read_to_string(&p).ok())
            .and_then(|s| toml::from_str::<FileConfig>(&s).ok())
            .unwrap_or_default();

        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));

        Config {
            daemon_url: daemon_url
                .or(file_config.daemon_url)
                .unwrap_or_else(|| "http://localhost:8080".to_string()),
            discriminator: discriminator
                .or(file_config.discriminator)
                .unwrap_or(3840),
            fabric_path: file_config
                .fabric_path
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".pentair").join("matter-fabrics.bin")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_no_args_no_file() {
        let config = Config::load(None, None, Some(PathBuf::from("/nonexistent")));
        assert_eq!(config.daemon_url, "http://localhost:8080");
        assert_eq!(config.discriminator, 3840);
    }

    #[test]
    fn cli_args_override_defaults() {
        let config = Config::load(
            Some("http://10.0.0.5:9090".to_string()),
            Some(1234),
            Some(PathBuf::from("/nonexistent")),
        );
        assert_eq!(config.daemon_url, "http://10.0.0.5:9090");
        assert_eq!(config.discriminator, 1234);
    }

    #[test]
    fn toml_file_parsed() {
        let dir = std::env::temp_dir().join("pentair-matter-test-config");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("matter.toml");
        std::fs::write(&path, r#"
            daemon_url = "http://192.168.1.50:8080"
            discriminator = 5555
        "#).unwrap();

        let config = Config::load(None, None, Some(path));
        assert_eq!(config.daemon_url, "http://192.168.1.50:8080");
        assert_eq!(config.discriminator, 5555);
    }
}
