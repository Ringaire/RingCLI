use ring_core::config::ResolvedConfig;
use ring_providers::factory;

fn main() {
    let config = ResolvedConfig::default();
    let bootstrap = factory::build_registry(&config);
    
    println!("=== Registry Status ===");
    println!("Providers count: {}", bootstrap.registry.list().len());
    println!("Default provider: {:?}", bootstrap.default_provider_id);
    
    println!("\n=== All Providers ===");
    for prov in bootstrap.registry.list() {
        println!("  - {}", prov.id());
    }
    
    println!("\n=== Test Lookup ===");
    if let Some(p) = bootstrap.registry.get("anthropic-s2a") {
        println!("Found 'anthropic-s2a': {}", p.id());
    } else {
        println!("NOT FOUND: anthropic-s2a");
    }
    
    if let Some(p) = bootstrap.registry.get("openai-sumooi") {
        println!("Found 'openai-sumooi': {}", p.id());
    } else {
        println!("NOT FOUND: openai-sumooi");
    }
}
