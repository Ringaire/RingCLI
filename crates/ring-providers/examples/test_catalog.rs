use ring_providers::catalog;
use std::path::PathBuf;

fn main() {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let path = PathBuf::from(home).join(".ring").join("config");
    let cat = catalog::load(Some(&path), None);
    
    println!("Loaded {} providers", cat.len());
    
    if let Some(entry) = cat.get("anthropic-s2a") {
        println!("\nanthropic-s2a:");
        println!("  name: {}", entry.name);
        println!("  kind: {:?}", entry.kind);
        println!("  base_url: {:?}", entry.base_url);
        println!("  api_key: {:?}", entry.api_key.as_ref().map(|k| format!("{}...", &k[..10])));
        println!("  api_key_env: {:?}", entry.api_key_env);
    } else {
        println!("\nanthropic-s2a NOT FOUND in catalog");
    }
    
    if let Some(entry) = cat.get("openai-sumooi") {
        println!("\nopenai-sumooi:");
        println!("  name: {}", entry.name);
        println!("  kind: {:?}", entry.kind);
        println!("  base_url: {:?}", entry.base_url);
        println!("  api_key: {:?}", entry.api_key.as_ref().map(|k| format!("{}...", &k[..10])));
        println!("  api_key_env: {:?}", entry.api_key_env);
    } else {
        println!("\nopenai-sumooi NOT FOUND in catalog");
    }
}
