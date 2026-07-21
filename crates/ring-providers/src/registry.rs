use std::collections::HashMap;
use std::sync::Arc;

use crate::provider::Provider;

pub struct ProviderRegistry {
    providers: HashMap<String, Arc<dyn Provider>>,
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self { providers: HashMap::new() }
    }

    pub fn register(&mut self, provider: impl Provider + 'static) {
        self.providers.insert(provider.id().to_string(), Arc::new(provider));
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.providers.get(id).cloned()
    }

    pub fn list(&self) -> Vec<Arc<dyn Provider>> {
        let mut v: Vec<Arc<dyn Provider>> = self.providers.values().cloned().collect();
        v.sort_by(|a, b| a.id().cmp(b.id()));
        v
    }

    pub fn contains(&self, id: &str) -> bool {
        self.providers.contains_key(id)
    }
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}
