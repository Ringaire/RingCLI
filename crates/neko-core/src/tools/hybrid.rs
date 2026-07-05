// ── HybridToolRegistry：混合模式工具注册表 ─────────────────────────────────

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use super::{Tool, ToolRegistry};

/// 混合模式工具注册表：内置工具 + 动态工具。
///
/// # 架构
///
/// - `builtin`：不可变 HashMap，启动时填充。读路径**无锁**，O(1) 查找。
/// - `dynamic`：`RwLock<HashMap>`，运行时注册（MCP 工具等）。读路径加读锁。
///
/// # 查找优先级
///
/// 1. 内置工具（builtin）：零锁开销
/// 2. 动态工具（dynamic）：读锁保护
///
/// # 注册语义
///
/// `register_arc` / `unregister` 仅作用于 `dynamic` 层——
/// 内置工具在构造时固化，运行时不可变，保证主工具集稳定性。
pub struct HybridToolRegistry {
    builtin: HashMap<String, Arc<dyn Tool>>,
    dynamic: RwLock<HashMap<String, Arc<dyn Tool>>>,
}

impl HybridToolRegistry {
    /// 创建空的混合注册表。
    ///
    /// 实际工具注册应通过 [`with_builtin_tools`] 或 `register_arc` 完成。
    pub fn new() -> Self {
        Self {
            builtin: HashMap::new(),
            dynamic: RwLock::new(HashMap::new()),
        }
    }

    /// 批量注册内置工具（构造期填充 builtin 层）。
    ///
    /// 工具名必须是 `&'static str`（来自 `BuiltinToolKind::name()` 的 const fn），
    /// 避免运行期字符串的生命周期问题。
    pub fn with_builtin_tools(
        mut self,
        tools: Vec<(&'static str, Arc<dyn Tool>)>,
    ) -> Self {
        for (name, tool) in tools {
            self.builtin.insert(name.to_string(), tool);
        }
        self
    }
}

impl Default for HybridToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry for HybridToolRegistry {
    fn register_arc(&self, tool: Arc<dyn Tool>) {
        let name = tool.name().to_string();
        self.dynamic.write().insert(name, tool);
    }

    fn unregister(&self, name: &str) {
        // 仅支持注销动态工具；内置工具不可变。
        self.dynamic.write().remove(name);
    }

    fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        // 1. 内置工具：无锁查找
        if let Some(tool) = self.builtin.get(name) {
            return Some(Arc::clone(tool));
        }
        // 2. 动态工具：读锁
        self.dynamic.read().get(name).cloned()
    }

    fn list(&self) -> Vec<Arc<dyn Tool>> {
        let mut out: Vec<Arc<dyn Tool>> = self.builtin.values().cloned().collect();
        out.extend(self.dynamic.read().values().cloned());
        out
    }
}

// ── 测试 ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ToolContext, ToolRegistry, ToolResult};
    use async_trait::async_trait;
    use serde_json::{json, Value};

    struct MockTool {
        name: &'static str,
    }

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str { self.name }
        fn description(&self) -> &str { "mock tool" }
        fn input_schema(&self) -> Value { json!({ "type": "object" }) }
        async fn execute(&self, _input: Value, _ctx: &ToolContext) -> ToolResult {
            ToolResult::ok_text(format!("executed: {}", self.name))
        }
    }

    #[test]
    fn test_hybrid_registry_builtin_lookup() {
        let registry = HybridToolRegistry::new().with_builtin_tools(vec![(
            "bash",
            Arc::new(MockTool { name: "bash" }) as Arc<dyn Tool>,
        )]);

        assert!(registry.get("bash").is_some());
        assert_eq!(registry.get("bash").unwrap().name(), "bash");
    }

    #[test]
    fn test_hybrid_registry_dynamic_registration() {
        let registry = HybridToolRegistry::new();
        registry.register_arc(Arc::new(MockTool { name: "custom" }));

        assert!(registry.get("custom").is_some());
        assert_eq!(registry.get("custom").unwrap().name(), "custom");
    }

    #[test]
    fn test_hybrid_registry_builtin_overrides_dynamic() {
        let registry = HybridToolRegistry::new().with_builtin_tools(vec![(
            "bash",
            Arc::new(MockTool { name: "bash_builtin" }) as Arc<dyn Tool>,
        )]);

        // 同名动态注册不应覆盖内置工具
        registry.register_arc(Arc::new(MockTool { name: "bash_dynamic" }));

        assert_eq!(registry.get("bash").unwrap().name(), "bash_builtin");
    }

    #[test]
    fn test_hybrid_registry_list_merges_both_layers() {
        let registry = HybridToolRegistry::new().with_builtin_tools(vec![
            ("bash", Arc::new(MockTool { name: "bash" }) as Arc<dyn Tool>),
            ("read", Arc::new(MockTool { name: "read" })),
        ]);
        registry.register_arc(Arc::new(MockTool { name: "custom" }));

        let tools = registry.list();
        assert_eq!(tools.len(), 3);

        let names: Vec<&str> = tools.iter().map(|t| t.name()).collect();
        assert!(names.contains(&"bash"));
        assert!(names.contains(&"read"));
        assert!(names.contains(&"custom"));
    }

    #[test]
    fn test_hybrid_registry_unregister_dynamic_only() {
        let registry = HybridToolRegistry::new().with_builtin_tools(vec![(
            "bash",
            Arc::new(MockTool { name: "bash" }) as Arc<dyn Tool>,
        )]);
        registry.register_arc(Arc::new(MockTool { name: "custom" }));

        registry.unregister("custom");
        assert!(registry.get("custom").is_none());

        // 内置工具不可注销
        registry.unregister("bash");
        assert!(registry.get("bash").is_some());
    }

    #[test]
    fn test_hybrid_registry_nonexistent_returns_none() {
        let registry = HybridToolRegistry::new();
        assert!(registry.get("nonexistent").is_none());
    }
}
