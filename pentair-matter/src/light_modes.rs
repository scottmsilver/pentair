/// Non-selectable modes that should be filtered from the ModeSelect picker.
const FILTERED_MODES: &[&str] = &["off", "on", "set", "sync"];

/// Maps IntelliBrite mode names to stable numeric indices for Matter ModeSelect.
#[derive(Debug, Clone)]
pub struct LightModeMap {
    modes: Vec<String>,
}

impl LightModeMap {
    pub fn from_available_modes(available: &[String]) -> Self {
        let modes: Vec<String> = available
            .iter()
            .filter(|m| !FILTERED_MODES.contains(&m.as_str()))
            .cloned()
            .collect();
        Self { modes }
    }

    pub fn name_by_index(&self, index: u8) -> Option<&str> {
        self.modes.get(index as usize).map(|s| s.as_str())
    }

    pub fn index_by_name(&self, name: &str) -> Option<u8> {
        self.modes.iter().position(|m| m == name).map(|i| i as u8)
    }

    pub fn current_mode_index(&self, mode: Option<&str>) -> Option<u8> {
        mode.and_then(|m| self.index_by_name(m))
    }

    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.modes.len()
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.modes.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (u8, &str)> {
        self.modes.iter().enumerate().map(|(i, m)| (i as u8, m.as_str()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn daemon_modes() -> Vec<String> {
        vec![
            "off", "on", "set", "sync", "swim", "party", "romantic",
            "caribbean", "american", "sunset", "royal", "blue", "green",
            "red", "white", "purple",
        ].into_iter().map(String::from).collect()
    }

    #[test]
    fn filters_non_selectable_modes() {
        let map = LightModeMap::from_available_modes(&daemon_modes());
        assert_eq!(map.len(), 12);
        assert!(map.index_by_name("off").is_none());
        assert!(map.index_by_name("on").is_none());
        assert!(map.index_by_name("set").is_none());
        assert!(map.index_by_name("sync").is_none());
    }

    #[test]
    fn stable_indices() {
        let map = LightModeMap::from_available_modes(&daemon_modes());
        assert_eq!(map.name_by_index(0), Some("swim"));
        assert_eq!(map.name_by_index(1), Some("party"));
        assert_eq!(map.name_by_index(3), Some("caribbean"));
        assert_eq!(map.index_by_name("caribbean"), Some(3));
    }

    #[test]
    fn current_mode_null() {
        let map = LightModeMap::from_available_modes(&daemon_modes());
        assert_eq!(map.current_mode_index(None), None);
    }

    #[test]
    fn current_mode_known() {
        let map = LightModeMap::from_available_modes(&daemon_modes());
        assert_eq!(map.current_mode_index(Some("caribbean")), Some(3));
    }

    #[test]
    fn round_trip() {
        let map = LightModeMap::from_available_modes(&daemon_modes());
        for (idx, name) in map.iter() {
            assert_eq!(map.name_by_index(idx), Some(name));
            assert_eq!(map.index_by_name(name), Some(idx));
        }
    }

    #[test]
    fn invalid_index() {
        let map = LightModeMap::from_available_modes(&daemon_modes());
        assert_eq!(map.name_by_index(255), None);
    }

    #[test]
    fn empty_modes() {
        let map = LightModeMap::from_available_modes(&[]);
        assert_eq!(map.len(), 0);
        assert!(map.is_empty());
        assert_eq!(map.current_mode_index(Some("party")), None);
    }
}
