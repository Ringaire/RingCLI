use ring_core::skills::{Skill, SkillRegistry};
use tracing::debug;

pub struct SkillsManager {
    pub registry: SkillRegistry,
}

impl SkillsManager {
    pub fn new() -> Self {
        Self { registry: SkillRegistry::new() }
    }

    pub fn register(&mut self, skill: Skill) {
        debug!(name = %skill.name, "registering skill");
        self.registry.register(skill);
    }

    pub fn unregister(&mut self, name: &str) {
        self.registry.unregister(name);
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.registry.get(name)
    }

    pub fn list(&self) -> Vec<&Skill> {
        self.registry.list()
    }
}

impl Default for SkillsManager {
    fn default() -> Self {
        Self::new()
    }
}
