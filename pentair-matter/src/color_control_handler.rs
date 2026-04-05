//! ColorControl cluster handler for IntelliBrite pool lights.
//!
//! Maps IntelliBrite light modes to hue/saturation color values so Google Home
//! shows a color picker. Implements HS+XY+CT features as required by Extended
//! Color Light (0x010D) device type.
//!
//! Internally everything is stored as hue/saturation. XY and CT reads/writes
//! are converted to/from HS.

use core::cell::Cell;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::sync::Arc;

use rs_matter::dm::{Cluster, Dataver, InvokeContext, ReadContext, WriteContext};
use rs_matter::error::Error;
use rs_matter::tlv::Nullable;
use rs_matter::with;

use crate::clusters::color_control::color_control::{self, *};
use crate::matter_bridge::{Command, SharedState};

/// IntelliBrite mode → (hue_0_254, saturation_0_254) mapping.
/// Hue: 0=red, 42=yellow, 85=green, 127=cyan, 170=blue, 212=magenta, 254=red
/// Saturation: 0=white, 254=full color
const MODE_COLORS: &[(&str, u8, u8)] = &[
    ("swim",      127, 180),  // cyan
    ("party",     212, 230),  // magenta/party
    ("romantic",  234, 150),  // pink
    ("caribbean", 118, 200),  // teal
    ("american",    0, 230),  // red
    ("sunset",     21, 230),  // orange
    ("royal",     191, 230),  // purple
    ("blue",      170, 254),  // blue
    ("green",      85, 254),  // green
    ("red",         0, 254),  // red
    ("white",       0,   0),  // white (no saturation)
    ("purple",    191, 254),  // purple
];

/// Find the nearest IntelliBrite mode for a given hue+saturation.
fn nearest_mode(hue: u8, sat: u8) -> &'static str {
    if sat < 30 {
        return "white";
    }
    let mut best_mode = "swim";
    let mut best_dist = i32::MAX;
    for &(mode, mh, ms) in MODE_COLORS {
        if mode == "white" { continue; }
        let dh = {
            let d = (hue as i32 - mh as i32).abs();
            d.min(254 - d)
        };
        let ds = (sat as i32 - ms as i32).abs();
        let dist = dh * dh + ds * ds;
        if dist < best_dist {
            best_dist = dist;
            best_mode = mode;
        }
    }
    best_mode
}

// --- Color space conversions ---
// Matter HS: hue 0-254 (maps to 0-360°), saturation 0-254 (maps to 0-1)
// Matter XY: CIE 1931 x,y as u16 (0-65535 maps to 0.0-1.0)
// Matter CT: color temp in mireds (reciprocal megakelvins)

fn hs_to_xy(hue: u8, sat: u8) -> (u16, u16) {
    // Convert Matter HS to RGB, then to CIE XY
    let h = hue as f64 / 254.0 * 360.0;
    let s = sat as f64 / 254.0;
    let v = 1.0; // full brightness

    // HSV to RGB
    let c = v * s;
    let hp = h / 60.0;
    let x_val = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let (r1, g1, b1) = match hp as u32 {
        0 => (c, x_val, 0.0),
        1 => (x_val, c, 0.0),
        2 => (0.0, c, x_val),
        3 => (0.0, x_val, c),
        4 => (x_val, 0.0, c),
        _ => (c, 0.0, x_val),
    };
    let m = v - c;
    let (r, g, b) = (r1 + m, g1 + m, b1 + m);

    // Gamma correction (sRGB → linear)
    let linearize = |c: f64| -> f64 {
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    let rl = linearize(r);
    let gl = linearize(g);
    let bl = linearize(b);

    // Linear RGB to CIE XYZ (sRGB D65 matrix)
    let x_cie = 0.4124 * rl + 0.3576 * gl + 0.1805 * bl;
    let y_cie = 0.2126 * rl + 0.7152 * gl + 0.0722 * bl;
    let z_cie = 0.0193 * rl + 0.1192 * gl + 0.9505 * bl;

    let sum = x_cie + y_cie + z_cie;
    if sum < 0.0001 {
        // White point D65
        return (20480, 21331); // 0.3127, 0.3290
    }

    let cx = x_cie / sum;
    let cy = y_cie / sum;

    ((cx * 65535.0) as u16, (cy * 65535.0) as u16)
}

fn xy_to_hs(x: u16, y: u16) -> (u8, u8) {
    let cx = x as f64 / 65535.0;
    let cy = y as f64 / 65535.0;

    // CIE xy to RGB (approximate, D65 reference)
    let cy_safe = if cy < 0.01 { 0.01 } else { cy };
    let z = 1.0 - cx - cy_safe;
    let y_cie = 1.0; // assume full brightness
    let x_cie = (y_cie / cy_safe) * cx;
    let z_cie = (y_cie / cy_safe) * z;

    // XYZ to linear RGB
    let r =  3.2406 * x_cie - 1.5372 * y_cie - 0.4986 * z_cie;
    let g = -0.9689 * x_cie + 1.8758 * y_cie + 0.0415 * z_cie;
    let b =  0.0557 * x_cie - 0.2040 * y_cie + 1.0570 * z_cie;

    // Clamp and convert to HSV
    let r = r.clamp(0.0, 1.0);
    let g = g.clamp(0.0, 1.0);
    let b = b.clamp(0.0, 1.0);

    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;

    let s = if max < 0.001 { 0.0 } else { delta / max };
    let h = if delta < 0.001 {
        0.0
    } else if (max - r).abs() < 0.001 {
        60.0 * (((g - b) / delta) % 6.0)
    } else if (max - g).abs() < 0.001 {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };
    let h = if h < 0.0 { h + 360.0 } else { h };

    ((h / 360.0 * 254.0) as u8, (s * 254.0) as u8)
}

fn hs_to_ct(hue: u8, sat: u8) -> u16 {
    if sat < 30 {
        return 370; // warm white ~2700K
    }
    // Map hue to a color temperature range
    // Blue (170) → cool (153 mireds = 6500K)
    // Red (0) → warm (500 mireds = 2000K)
    let h = hue as f64 / 254.0 * 360.0;
    let mireds = 500.0 - (h / 360.0) * 347.0; // range 153-500
    mireds.clamp(153.0, 500.0) as u16
}

fn ct_to_hs(mireds: u16) -> (u8, u8) {
    // Color temperature → white mode (low saturation maps to "white" in nearest_mode)
    let _ = mireds; // CT is irrelevant for IntelliBrite — all CT writes map to white
    (0, 0)
}

pub struct ColorControlHandler {
    dataver: Dataver,
    shared: Arc<SharedState>,
    cmd_tx: mpsc::Sender<Command>,
    current_hue: Cell<u8>,
    current_saturation: Cell<u8>,
    last_gen: Cell<u64>,
    /// Suppress sync_from_shared after a local color write until daemon confirms.
    color_written_locally: Cell<bool>,
    color_write_gen: Cell<u64>,
}

impl ColorControlHandler {
    pub fn new(
        dataver: Dataver,
        shared: Arc<SharedState>,
        cmd_tx: mpsc::Sender<Command>,
    ) -> Self {
        let (hue, sat, gen) = {
            let s = shared.state.lock().unwrap();
            let (h, s_val) = s.light_mode_name.as_deref()
                .and_then(|name| MODE_COLORS.iter().find(|&&(n, _, _)| n == name))
                .map(|&(_, h, s)| (h, s))
                .unwrap_or((0, 0));
            (h, s_val, shared.generation.load(Ordering::Acquire))
        };
        Self {
            dataver,
            shared,
            cmd_tx,
            current_hue: Cell::new(hue),
            current_saturation: Cell::new(sat),
            last_gen: Cell::new(gen),
            color_written_locally: Cell::new(false),
            color_write_gen: Cell::new(0),
        }
    }

    pub const fn adapt(self) -> color_control::HandlerAdaptor<Self> {
        color_control::HandlerAdaptor(self)
    }

    fn sync_from_shared(&self) {
        let current_gen = self.shared.generation.load(Ordering::Acquire);
        if current_gen != self.last_gen.get() {
            self.last_gen.set(current_gen);
            let s = self.shared.state.lock().unwrap();
            if let Some(mode_name) = s.light_mode_name.as_deref() {
                if self.color_written_locally.get() {
                    // Check if daemon caught up to our local write
                    let expected_mode = nearest_mode(self.current_hue.get(), self.current_saturation.get());
                    if mode_name == expected_mode {
                        self.color_written_locally.set(false);
                    } else if current_gen - self.color_write_gen.get() > 10 {
                        // Timed out — resume daemon sync
                        self.color_written_locally.set(false);
                    }
                }
                if !self.color_written_locally.get() {
                    if let Some(&(_, h, s_val)) = MODE_COLORS.iter().find(|&&(name, _, _)| name == mode_name) {
                        self.current_hue.set(h);
                        self.current_saturation.set(s_val);
                    }
                }
            }
        }
    }

    fn apply_color(&self, hue: u8, sat: u8) {
        self.current_hue.set(hue);
        self.current_saturation.set(sat);
        self.color_written_locally.set(true);
        self.color_write_gen.set(self.shared.generation.load(Ordering::Acquire));
        self.dataver.changed();
        let mode = nearest_mode(hue, sat);
        if let Err(e) = self.cmd_tx.send(Command::SetLightMode(mode.to_string())) {
            tracing::error!("Failed to send light mode command: {e}");
        }
    }
}

impl color_control::ClusterHandler for ColorControlHandler {
    // Features: HS (0x01) + XY (0x08) + CT (0x10) = 0x19
    // EnhancedHue (0x02) removed — we don't implement 16-bit hue commands,
    // and Google Home may send enhanced commands if the feature is declared.
    const CLUSTER: Cluster<'static> = color_control::FULL_CLUSTER
        .with_revision(7)
        .with_features(0x19) // HS + XY + CT
        .with_attrs(with!(
            required;
            color_control::AttributeId::CurrentHue
                | color_control::AttributeId::CurrentSaturation
                | color_control::AttributeId::CurrentX
                | color_control::AttributeId::CurrentY
                | color_control::AttributeId::ColorTemperatureMireds
                | color_control::AttributeId::ColorCapabilities
                | color_control::AttributeId::EnhancedColorMode
                | color_control::AttributeId::RemainingTime
                | color_control::AttributeId::ColorTempPhysicalMinMireds
                | color_control::AttributeId::ColorTempPhysicalMaxMireds
        ))
        .with_cmds(with!(
            color_control::CommandId::MoveToHue
                | color_control::CommandId::MoveToSaturation
                | color_control::CommandId::MoveToHueAndSaturation
                | color_control::CommandId::MoveToColor
                | color_control::CommandId::MoveToColorTemperature
                | color_control::CommandId::StopMoveStep
        ));

    fn dataver(&self) -> u32 { self.dataver.get() }
    fn dataver_changed(&self) { self.dataver.changed(); }

    // --- HS attributes ---
    fn current_hue(&self, _ctx: impl ReadContext) -> Result<u8, Error> {
        self.sync_from_shared();
        Ok(self.current_hue.get())
    }
    fn current_saturation(&self, _ctx: impl ReadContext) -> Result<u8, Error> {
        self.sync_from_shared();
        Ok(self.current_saturation.get())
    }

    // --- XY attributes (derived from HS) ---
    fn current_x(&self, _ctx: impl ReadContext) -> Result<u16, Error> {
        self.sync_from_shared();
        let (x, _) = hs_to_xy(self.current_hue.get(), self.current_saturation.get());
        Ok(x)
    }
    fn current_y(&self, _ctx: impl ReadContext) -> Result<u16, Error> {
        self.sync_from_shared();
        let (_, y) = hs_to_xy(self.current_hue.get(), self.current_saturation.get());
        Ok(y)
    }

    // --- CT attributes (derived from HS) ---
    fn color_temperature_mireds(&self, _ctx: impl ReadContext) -> Result<u16, Error> {
        self.sync_from_shared();
        Ok(hs_to_ct(self.current_hue.get(), self.current_saturation.get()))
    }
    fn color_temp_physical_min_mireds(&self, _ctx: impl ReadContext) -> Result<u16, Error> {
        Ok(153) // ~6500K (cool white)
    }
    fn color_temp_physical_max_mireds(&self, _ctx: impl ReadContext) -> Result<u16, Error> {
        Ok(500) // ~2000K (warm)
    }

    // --- Common attributes ---
    fn remaining_time(&self, _ctx: impl ReadContext) -> Result<u16, Error> {
        Ok(0) // Transitions are instant
    }
    fn color_mode(&self, _ctx: impl ReadContext) -> Result<ColorModeEnum, Error> {
        Ok(ColorModeEnum::CurrentHueAndCurrentSaturation)
    }
    fn enhanced_color_mode(&self, _ctx: impl ReadContext) -> Result<EnhancedColorModeEnum, Error> {
        Ok(EnhancedColorModeEnum::CurrentHueAndCurrentSaturation)
    }
    fn color_capabilities(&self, _ctx: impl ReadContext) -> Result<ColorCapabilitiesBitmap, Error> {
        // HS + XY + CT
        Ok(ColorCapabilitiesBitmap::HUE_SATURATION | ColorCapabilitiesBitmap::XY | ColorCapabilitiesBitmap::COLOR_TEMPERATURE)
    }
    fn options(&self, _ctx: impl ReadContext) -> Result<OptionsBitmap, Error> {
        Ok(OptionsBitmap::empty())
    }
    fn set_options(&self, _ctx: impl WriteContext, _value: OptionsBitmap) -> Result<(), Error> {
        Ok(())
    }
    fn number_of_primaries(&self, _ctx: impl ReadContext) -> Result<Nullable<u8>, Error> {
        Ok(Nullable::some(0))
    }

    // --- HS Commands ---
    fn handle_move_to_hue(&self, _ctx: impl InvokeContext, req: MoveToHueRequest<'_>) -> Result<(), Error> {
        self.apply_color(req.hue()?, self.current_saturation.get());
        Ok(())
    }
    fn handle_move_to_saturation(&self, _ctx: impl InvokeContext, req: MoveToSaturationRequest<'_>) -> Result<(), Error> {
        self.apply_color(self.current_hue.get(), req.saturation()?);
        Ok(())
    }
    fn handle_move_to_hue_and_saturation(&self, _ctx: impl InvokeContext, req: MoveToHueAndSaturationRequest<'_>) -> Result<(), Error> {
        self.apply_color(req.hue()?, req.saturation()?);
        Ok(())
    }

    // --- XY Command (convert to HS) ---
    fn handle_move_to_color(&self, _ctx: impl InvokeContext, req: MoveToColorRequest<'_>) -> Result<(), Error> {
        let (h, s) = xy_to_hs(req.color_x()?, req.color_y()?);
        self.apply_color(h, s);
        Ok(())
    }

    // --- CT Command (convert to HS) ---
    fn handle_move_to_color_temperature(&self, _ctx: impl InvokeContext, req: MoveToColorTemperatureRequest<'_>) -> Result<(), Error> {
        let (h, s) = ct_to_hs(req.color_temperature_mireds()?);
        self.apply_color(h, s);
        Ok(())
    }

    // --- No-op commands ---
    fn handle_move_hue(&self, _ctx: impl InvokeContext, _req: MoveHueRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_step_hue(&self, _ctx: impl InvokeContext, _req: StepHueRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_move_saturation(&self, _ctx: impl InvokeContext, _req: MoveSaturationRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_step_saturation(&self, _ctx: impl InvokeContext, _req: StepSaturationRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_move_color(&self, _ctx: impl InvokeContext, _req: MoveColorRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_step_color(&self, _ctx: impl InvokeContext, _req: StepColorRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_stop_move_step(&self, _ctx: impl InvokeContext, _req: StopMoveStepRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_enhanced_move_to_hue(&self, _ctx: impl InvokeContext, _req: EnhancedMoveToHueRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_enhanced_move_hue(&self, _ctx: impl InvokeContext, _req: EnhancedMoveHueRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_enhanced_step_hue(&self, _ctx: impl InvokeContext, _req: EnhancedStepHueRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_enhanced_move_to_hue_and_saturation(&self, _ctx: impl InvokeContext, _req: EnhancedMoveToHueAndSaturationRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_color_loop_set(&self, _ctx: impl InvokeContext, _req: ColorLoopSetRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_move_color_temperature(&self, _ctx: impl InvokeContext, _req: MoveColorTemperatureRequest<'_>) -> Result<(), Error> { Ok(()) }
    fn handle_step_color_temperature(&self, _ctx: impl InvokeContext, _req: StepColorTemperatureRequest<'_>) -> Result<(), Error> { Ok(()) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_mode_finds_correct_colors() {
        assert_eq!(nearest_mode(170, 254), "blue");
        assert_eq!(nearest_mode(85, 254), "green");
        assert_eq!(nearest_mode(0, 254), "red");
        assert_eq!(nearest_mode(0, 0), "white");
        assert_eq!(nearest_mode(127, 180), "swim");
        assert_eq!(nearest_mode(118, 200), "caribbean");
    }

    #[test]
    fn low_saturation_maps_to_white() {
        assert_eq!(nearest_mode(170, 10), "white");
        assert_eq!(nearest_mode(85, 5), "white");
    }

    #[test]
    fn hs_xy_roundtrip_preserves_hue_region() {
        // Blue should stay in blue region after roundtrip
        let (x, y) = hs_to_xy(170, 254);
        let (h, _s) = xy_to_hs(x, y);
        assert!((h as i32 - 170).abs() < 20, "blue roundtrip: got hue {h}");
    }

    #[test]
    fn hue_wrapping_at_254_maps_to_red() {
        assert_eq!(nearest_mode(254, 254), "red");
        assert_eq!(nearest_mode(253, 254), "red");
    }

    #[test]
    fn xy_zero_does_not_panic() {
        let (h, s) = xy_to_hs(0, 0);
        assert!(h <= 254);
        assert!(s <= 254);
    }

    #[test]
    fn hs_xy_roundtrip_red() {
        let (x, y) = hs_to_xy(0, 254);
        let (h, _) = xy_to_hs(x, y);
        // Hue 0 and 254 are both red (wrapping)
        assert!(h < 20 || h > 240, "red roundtrip: got hue {h}");
    }

    #[test]
    fn hs_xy_roundtrip_green() {
        let (x, y) = hs_to_xy(85, 254);
        let (h, _) = xy_to_hs(x, y);
        assert!((h as i32 - 85).abs() < 20, "green roundtrip: got hue {h}");
    }

    #[test]
    fn ct_range_is_valid() {
        // Red → warm, Blue → cool
        let warm = hs_to_ct(0, 254);
        let cool = hs_to_ct(170, 254);
        assert!(warm > cool, "red should be warmer (higher mireds) than blue");
        assert!(warm <= 500);
        assert!(cool >= 153);
    }
}
