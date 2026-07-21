use ring_core::config::ResolvedConfig;
use ring_providers::{catalog, factory};

fn main() {
    println!("=== Loading catalog ===");
    let global_dir = ring_core::session::paths::config_dir();
    let project_dir = std::env::current_dir().ok();
    let cat = catalog::load(Some(&global_dir), project_dir.as_deref());
    
    println!("Catalog has {} providers", cat.len());
    if let Some(entry) = cat.get("anthropic-s2a") {
        println!("  anthropic-s2a: api_key={:?}, api_key_env={:?}",
            entry.api_key.as_ref().map(|k| k.len()),
            entry.api_key_env
        );
    }
    
    println!("\n=== Building registry ===");
    let config = ResolvedConfig::default();
    let bootstrap = factory::build_registry(&config);
    
    println!("\nRegistry has {} providers", bootstrap.registry.list().len());
    for prov in bootstrap.registry.list() {
        println!("  - {}", prov.id());
    }
}
