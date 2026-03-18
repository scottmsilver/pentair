use crate::backend::Backend;
use crate::output;

pub async fn show(backend: &mut Backend, json: bool) -> Result<(), Box<dyn std::error::Error>> {
    let scg = backend.get_scg_config().await?;

    if json {
        output::print_json(&scg);
        return Ok(());
    }

    println!("Chlorinator:");
    println!("  Installed:        {}", if scg.installed { "Yes" } else { "No" });
    println!("  Status:           {}", scg.status);
    println!("  Pool Set Point:   {}%", scg.pool_set_point);
    println!("  Spa Set Point:    {}%", scg.spa_set_point);
    println!("  Salt:             {} ppm", scg.salt_ppm);
    println!("  Super Chlor Timer:{}", scg.super_chlor_timer);

    Ok(())
}

pub async fn set(
    backend: &mut Backend,
    pool: i32,
    spa: i32,
    json: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    backend.set_scg_config(pool, spa).await?;

    if json {
        output::print_json(&serde_json::json!({
            "pool_output": pool,
            "spa_output": spa,
        }));
    } else {
        println!("Chlorinator set to pool={}%, spa={}%", pool, spa);
    }

    Ok(())
}
