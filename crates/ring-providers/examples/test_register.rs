use ring_providers::{catalog, factory};
use ring_core::config::ResolvedConfig;

fn main() {
    let global_dir = ring_core::session::paths::config_dir();
    let project_dir = std::env::current_dir().ok();
    let cat = catalog::load(Some(&global_dir), project_dir.as_deref());
    
    println!("=== Checking anthropic-s2a ===");
    if let Some(entry) = cat.get("anthropic-s2a") {
        println!("Entry found in catalog:");
        println!("  name: {}", entry.name);
        println!("  kind: {:?}", entry.kind);
        println!("  base_url: {:?}", entry.base_url);
        println!(
            "  api_key: {:?}",
            entry.api_key.as_ref().map(|k| format!("{}...", k.chars().take(10).collect::<String>()))
        );
        println!("  api_key_env: {:?}", entry.api_key_env);
        
        // 模拟 factory 逻辑
        let api_key = entry.api_key.clone()
            .filter(|k| !k.trim().is_empty())
            .or_else(|| entry.api_key_env.as_deref().and_then(|env| std::env::var(env).ok()))
            .unwrap_or_default();
        
        let is_builtin_needs_key = entry.api_key_env.is_some();
        
        println!("\nFactory logic:");
        println!("  api_key length: {}", api_key.len());
        println!("  is_builtin_needs_key: {}", is_builtin_needs_key);
        println!("  would skip: {}", is_builtin_needs_key && api_key.is_empty());
    } else {
        println!("NOT FOUND in catalog!");
    }
    
    println!("\n=== Building registry ===");
    let config = ResolvedConfig::default();
    let bootstrap = factory::build_registry(&config);
    
    println!("Registry providers:");
    for prov in bootstrap.registry.list() {
        println!("  - {}", prov.id());
    }
    
    if let Some(prov) = bootstrap.registry.get("anthropic-s2a") {
        println!("\nFound {} in registry!", prov.id());
    } else {
        println!("\nanthropic-s2a NOT in registry");
    }
}
