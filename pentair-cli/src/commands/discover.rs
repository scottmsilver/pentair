use pentair_client::discovery::discover_all;

use crate::output;

pub async fn run(json: bool) -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("Searching for adapters...");
    let adapters = discover_all().await?;

    if json {
        output::print_json(&adapters);
        return Ok(());
    }

    println!("Found {} adapter(s):", adapters.len());
    for adapter in &adapters {
        let ip = format!(
            "{}.{}.{}.{}",
            adapter.ip[0], adapter.ip[1], adapter.ip[2], adapter.ip[3]
        );
        println!();
        println!("  Name:    {}", adapter.adapter_name);
        println!("  Address: {}:{}", ip, adapter.port);
        println!(
            "  Type:    {}/{}",
            adapter.gateway_type, adapter.gateway_subtype
        );
    }

    Ok(())
}
