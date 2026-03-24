use std::fmt;

/// Bitfield describing installed equipment on the controller.
///
/// Each bit indicates the presence of a specific piece of hardware.
/// The raw value comes from the equipment-config response.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct EquipmentFlags(u32);

impl EquipmentFlags {
    /// Wrap a raw u32 value from the protocol.
    pub fn from_raw(value: u32) -> Self {
        Self(value)
    }

    /// Return the underlying u32.
    pub fn raw(&self) -> u32 {
        self.0
    }

    fn bit(&self, n: u32) -> bool {
        (self.0 >> n) & 1 != 0
    }

    /// Bit 0 -- solar heating panels installed.
    pub fn has_solar(&self) -> bool {
        self.bit(0)
    }

    /// Bit 1 -- solar acts as a heat pump.
    pub fn has_solar_as_heat_pump(&self) -> bool {
        self.bit(1)
    }

    /// Bit 2 -- salt chlorine generator present.
    pub fn has_chlorinator(&self) -> bool {
        self.bit(2)
    }

    /// Bit 3 (0x08) -- IntelliBrite LED lights.
    pub fn has_intellibrite(&self) -> bool {
        self.bit(3)
    }

    /// Bit 4 (0x10) -- IntelliFlow pump #0.
    pub fn has_intelliflo_0(&self) -> bool {
        self.bit(4)
    }

    /// Bit 5 -- IntelliFlow pump #1.
    pub fn has_intelliflo_1(&self) -> bool {
        self.bit(5)
    }

    /// Bit 6 -- IntelliFlow pump #2.
    pub fn has_intelliflo_2(&self) -> bool {
        self.bit(6)
    }

    /// Bit 7 -- IntelliFlow pump #3.
    pub fn has_intelliflo_3(&self) -> bool {
        self.bit(7)
    }

    /// Bit 8 -- IntelliFlow pump #4.
    pub fn has_intelliflo_4(&self) -> bool {
        self.bit(8)
    }

    /// Bit 9 -- IntelliFlow pump #5.
    pub fn has_intelliflo_5(&self) -> bool {
        self.bit(9)
    }

    /// Bit 10 -- IntelliFlow pump #6.
    pub fn has_intelliflo_6(&self) -> bool {
        self.bit(10)
    }

    /// Bit 11 -- IntelliFlow pump #7.
    pub fn has_intelliflo_7(&self) -> bool {
        self.bit(11)
    }

    /// Bit 12 -- IntelliChem chemistry controller.
    pub fn has_intellichem(&self) -> bool {
        self.bit(12)
    }

    /// Bit 13 -- hybrid heater (gas + heat-pump combo).
    pub fn has_hybrid_heater(&self) -> bool {
        self.bit(13)
    }

    /// Bit 14 -- MaxHyReach (max hydraulic reach / plumbing).
    pub fn has_max_hy_reach(&self) -> bool {
        self.bit(14)
    }

    /// Bit 15 -- UltraTemp heat pump.
    pub fn has_ultra_temp(&self) -> bool {
        self.bit(15)
    }
}

impl fmt::Display for EquipmentFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let flags: &[(&str, fn(&EquipmentFlags) -> bool)] = &[
            ("Solar", EquipmentFlags::has_solar),
            ("SolarAsHeatPump", EquipmentFlags::has_solar_as_heat_pump),
            ("Chlorinator", EquipmentFlags::has_chlorinator),
            ("IntelliBrite", EquipmentFlags::has_intellibrite),
            ("IntelliFlow0", EquipmentFlags::has_intelliflo_0),
            ("IntelliFlow1", EquipmentFlags::has_intelliflo_1),
            ("IntelliFlow2", EquipmentFlags::has_intelliflo_2),
            ("IntelliFlow3", EquipmentFlags::has_intelliflo_3),
            ("IntelliFlow4", EquipmentFlags::has_intelliflo_4),
            ("IntelliFlow5", EquipmentFlags::has_intelliflo_5),
            ("IntelliFlow6", EquipmentFlags::has_intelliflo_6),
            ("IntelliFlow7", EquipmentFlags::has_intelliflo_7),
            ("IntelliChem", EquipmentFlags::has_intellichem),
            ("HybridHeater", EquipmentFlags::has_hybrid_heater),
            ("MaxHyReach", EquipmentFlags::has_max_hy_reach),
            ("UltraTemp", EquipmentFlags::has_ultra_temp),
        ];

        let mut first = true;
        for (name, check) in flags {
            if check(self) {
                if !first {
                    write!(f, " | ")?;
                }
                write!(f, "{}", name)?;
                first = false;
            }
        }

        if first {
            write!(f, "(none)")?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real hardware value 24 (0x18) = IntelliBrite + IntelliFlow0.
    #[test]
    fn live_hardware_flags_24() {
        let flags = EquipmentFlags::from_raw(0x18);
        assert_eq!(flags.raw(), 24);

        // These two should be set
        assert!(flags.has_intellibrite(), "bit 3 should be set");
        assert!(flags.has_intelliflo_0(), "bit 4 should be set");

        // Everything else should be clear
        assert!(!flags.has_solar());
        assert!(!flags.has_solar_as_heat_pump());
        assert!(!flags.has_chlorinator());
        assert!(!flags.has_intelliflo_1());
        assert!(!flags.has_intelliflo_2());
        assert!(!flags.has_intelliflo_3());
        assert!(!flags.has_intelliflo_4());
        assert!(!flags.has_intelliflo_5());
        assert!(!flags.has_intelliflo_6());
        assert!(!flags.has_intelliflo_7());
        assert!(!flags.has_intellichem());
        assert!(!flags.has_hybrid_heater());
        assert!(!flags.has_max_hy_reach());
        assert!(!flags.has_ultra_temp());
    }

    #[test]
    fn all_flags_set() {
        let flags = EquipmentFlags::from_raw(0xFFFF);
        assert!(flags.has_solar());
        assert!(flags.has_solar_as_heat_pump());
        assert!(flags.has_chlorinator());
        assert!(flags.has_intellibrite());
        assert!(flags.has_intelliflo_0());
        assert!(flags.has_intelliflo_1());
        assert!(flags.has_intelliflo_2());
        assert!(flags.has_intelliflo_3());
        assert!(flags.has_intelliflo_4());
        assert!(flags.has_intelliflo_5());
        assert!(flags.has_intelliflo_6());
        assert!(flags.has_intelliflo_7());
        assert!(flags.has_intellichem());
        assert!(flags.has_hybrid_heater());
        assert!(flags.has_max_hy_reach());
        assert!(flags.has_ultra_temp());
    }

    #[test]
    fn no_flags_set() {
        let flags = EquipmentFlags::from_raw(0);
        assert!(!flags.has_solar());
        assert!(!flags.has_intellibrite());
        assert_eq!(format!("{}", flags), "(none)");
    }

    #[test]
    fn display_format() {
        let flags = EquipmentFlags::from_raw(0x18);
        assert_eq!(format!("{}", flags), "IntelliBrite | IntelliFlow0");
    }

    #[test]
    fn individual_bits() {
        for bit in 0..16u32 {
            let flags = EquipmentFlags::from_raw(1 << bit);
            // Exactly one flag should be set
            let checks: Vec<bool> = vec![
                flags.has_solar(),
                flags.has_solar_as_heat_pump(),
                flags.has_chlorinator(),
                flags.has_intellibrite(),
                flags.has_intelliflo_0(),
                flags.has_intelliflo_1(),
                flags.has_intelliflo_2(),
                flags.has_intelliflo_3(),
                flags.has_intelliflo_4(),
                flags.has_intelliflo_5(),
                flags.has_intelliflo_6(),
                flags.has_intelliflo_7(),
                flags.has_intellichem(),
                flags.has_hybrid_heater(),
                flags.has_max_hy_reach(),
                flags.has_ultra_temp(),
            ];
            let set_count = checks.iter().filter(|&&c| c).count();
            assert_eq!(set_count, 1, "bit {} should set exactly one flag", bit);
            assert!(
                checks[bit as usize],
                "bit {} should set the correct flag",
                bit
            );
        }
    }
}
