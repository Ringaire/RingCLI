use ring_core::config::ResolvedConfig;

fn main() {
    let config = ResolvedConfig::default();
    let bootstrap = ring_providers::factory::build_registry(&config);
    
    println!("Registry contains {} providers", bootstrap.registry.list().len());
    println!("Default provider: {:?}", bootstrap.default_provider_id);
    
    for prov in bootstrap.registry.list() {
        println!("  - {}", prov.id());
    }
}
