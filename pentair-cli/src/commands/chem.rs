use crate::backend::Backend;
use crate::output;

pub async fn run(backend: &mut Backend, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let chem = backend.get_chem_data().await?;

    if json {
        output::print_json(&chem);
        return Ok(());
    }

    if !chem.is_valid {
        println!("Chemistry data not available (no IntelliChem detected)");
        return Ok(());
    }

    println!("Chemistry:");
    println!(
        "  pH:             {:.2} (set point: {:.2})",
        chem.ph, chem.ph_set_point
    );
    println!(
        "  ORP:            {} mV (set point: {} mV)",
        chem.orp, chem.orp_set_point
    );
    println!("  Saturation:     {:.2}", chem.saturation);
    println!("  Salt:           {} ppm", chem.salt_ppm);
    println!("  Water Temp:     {}", chem.water_temp);
    println!("  Calcium:        {} ppm", chem.calcium);
    println!("  Cyanuric Acid:  {} ppm", chem.cyanuric_acid);
    println!("  Alkalinity:     {} ppm", chem.alkalinity);
    println!();
    println!("Dosing:");
    println!("  pH Dose Time:   {}s", chem.ph_dose_time);
    println!("  ORP Dose Time:  {}s", chem.orp_dose_time);
    println!("  pH Dose Volume: {} mL", chem.ph_dose_volume);
    println!("  ORP Dose Volume:{} mL", chem.orp_dose_volume);
    println!("  pH Supply:      {}", chem.ph_supply_level);
    println!("  ORP Supply:     {}", chem.orp_supply_level);
    println!();
    println!("Firmware: {}.{}", chem.fw_major, chem.fw_minor);

    Ok(())
}
