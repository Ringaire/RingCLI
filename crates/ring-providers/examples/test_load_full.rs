use ring_providers::catalog;
use std::path::PathBuf;

fn main() {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let global_dir = PathBuf::from(&home).join(".ring").join("config");
    let project_dir = Some(PathBuf::from("/mnt/data/Projects/Ringaire/Neko-all/ringcli"));
    
    println!("Loading catalog:");
    println!("  global_dir: {:?}", global_dir);
    println!("  project_dir: {:?}", project_dir);
    
    let cat = catalog::load(Some(&global_dir), project_dir.as_deref());
    
    println!("\nLoaded {} providers:", cat.len());
    for (id, entry) in &cat {
        if id.contains("anthropic") || id.contains("openai") {
            println!("  {}: api_key={:?}, api_key_env={:?}",
                id,
                entry.api_key.as_ref().map(|k| format!("{}...", &k[..10.min(k.len())])),
                entry.api_key_env
            );
        }
    }
}
