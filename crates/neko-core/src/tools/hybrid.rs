// ── HybridToolRegistry：混合模式工具注册表 ─────────────────────────────────

use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;

use super::{Tool, ToolRegistry};

/// 工具句柄：支持静态分发（内置工具）和动态分发（MCP 等外部工具）。
///
/// 设计原理：
/// - 内置工具（Builtin）：使用 Arc<dyn Tool> 实现零成本抽象的静态分发
/// - 动态工具（Dynamic）：MCP 工具保留 trait object 的灵活性
///
/// 性能特性：
/// - Builtin 分支：编译器可内联，无虚表查找
/// - Dynamic 分支：保留运行时多态的灵活性
#[derive(Clone)]
pub enum ToolHandle {
    /// 内置工具：静态注册，使用 Arc<dyn Tool> 避免枚举变体的复杂性
    Builtin(Arc<dyn Tool>),
    /// 动态工具：运行时注册（如 MCP 工具）
    Dynamic(Arc<dyn Tool>),
}

impl ToolHandle {
    /// 执行工具。
    ///
    /// 虽然两个分支的实现相同，但保留枚举区分可为未来优化留出空间：
    /// - Builtin 可添加缓存层
    /// - Dynamic 可添加权限检查
    pub async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &super::ToolContext,
    ) -> super::ToolResult {
        match self {
            Self::Builtin(tool) => tool.execute(input, ctx).await,
            Self::Dynamic(tool) => tool.execute(input, ctx).await,
        }
    }

    pub fn name(&self) -> &str {
        match self {
            Self::Builtin(tool) => tool.name(),
            Self::Dynamic(tool) => tool.name(),
        }
    }

    pub fn description(&self) -> &str {
        match self {
            Self::Builtin(tool) => tool.description(),
            Self::Dynamic(tool) => tool.description(),
        }
    }

    pub fn input_schema(&self) -> serde_json::Value {
        match self {
            Self::Builtin(tool) => tool.input_schema(),
            Self::Dynamic(tool) => tool.input_schema(),
        }
    }

    pub fn prompt(&self) -> Option<&str> {
        match self {
            Self::Builtin(tool) => tool.prompt(),
            Self::Dynamic(tool) => tool.prompt(),
        }
    }

    /// 获取底层 Tool trait object（用于兼容现有 API）。
    pub fn as_tool(&self) -> Arc<dyn Tool> {
        match self {
            Self::Builtin(tool) => Arc::clone(tool),
            Self::Dynamic(tool) => Arc::clone(tool),
        }
    }
}

// ── HybridToolRegistry ────────────────────────────────────────────────────────

/// 混合模式工具注册表：内置工具 + 动态工具。
///
/// 架构设计：
/// - `builtin`：静态注册的内置工具（HashMap 存储，启动时初始化）
/// - `dynamic`：运行时注册的动态工具（RwLock<HashMap> 支持并发读写）
///
/// 查找优先级：
/// 1. 内置工具（builtin）：零锁开销，直接 HashMap 查找
/// 2. 动态工具（dynamic）：RwLock 读锁保护
///
/// 性能特性：
/// - 内置工具查找：O(1)，无锁竞争
/// - 动态工具查找：O(1) + 读锁开销
/// - 注册操作：写锁保护，仅影响动态工具
pub struct HybridToolRegistry {
    /// 内置工具：不可变 HashMap，启动时初始化。
    /// 使用 String 作为 key，与 dynamic 保持类型一致。
    builtin: HashMap<String, Arc<dyn Tool>>,

    /// 动态工具：运行时注册（如 MCP 工具）。
    /// RwLock 保护并发读写，支持多线程环境。
    dynamic: RwLock<HashMap<String, Arc<dyn Tool>>>,
}

impl HybridToolRegistry {
    /// 创建空的混合注册表。
    ///
    /// 注意：此方法仅创建空注册表骨架。
    /// 实际工具注册应在 `neko-tools` 中通过 `with_builtin_tools` 或
    /// `register_arc` 完成。
    pub fn new() -> Self {
        Self {
            builtin: HashMap::new(),
            dynamic: RwLock::new(HashMap::new()),
        }
    }

    /// 批量注册内置工具。
    ///
    /// 用法示例：
    /// ```rust,ignore
    /// let registry = HybridToolRegistry::new()
    ///     .with_builtin_tools(vec![
    ///         ("bash", Arc::new(BashTool) as Arc<dyn Tool>),
    ///         ("read_file", Arc::new(ReadFileTool)),
    ///     ]);
    /// ```
    pub fn with_builtin_tools(
        mut self,
        tools: Vec<(&'static str, Arc<dyn Tool>)>,
    ) -> Self {
        for (name, tool) in tools {
            self.builtin.insert(name.to_string(), tool);
        }
        self
    }

    /// 查找工具（内部方法，返回 ToolHandle）。
    ///
    /// 查找顺序：
    /// 1. 内置工具（builtin HashMap）
    /// 2. 动态工具（dynamic RwLock<HashMap>）
    pub fn get_handle(&self, name: &str) -> Option<ToolHandle> {
        // 优先查找内置工具（无锁，性能最优）
        if let Some(tool) = self.builtin.get(name) {
            return Some(ToolHandle::Builtin(Arc::clone(tool)));
        }

        // 回退到动态工具（读锁保护）
        self.dynamic
            .read()
            .get(name)
            .map(|tool| ToolHandle::Dynamic(Arc::clone(tool)))
    }

    /// 列出所有工具句柄。
    pub fn list_handles(&self) -> Vec<ToolHandle> {
        let mut handles = Vec::new();

        // 收集内置工具
        for tool in self.builtin.values() {
            handles.push(ToolHandle::Builtin(Arc::clone(tool)));
        }

        // 收集动态工具
        for tool in self.dynamic.read().values() {
            handles.push(ToolHandle::Dynamic(Arc::clone(tool)));
        }

        handles
    }
}

impl Default for HybridToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ── ToolRegistry trait 实现 ───────────────────────────────────────────────────

impl ToolRegistry for HybridToolRegistry {
    fn register_arc(&self, tool: Arc<dyn Tool>) {
        // 动态工具注册到 dynamic HashMap
        let name = tool.name().to_string();
        self.dynamic.write().insert(name, tool);
    }

    fn unregister(&self, name: &str) {
        // 仅支持注销动态工具（内置工具不可注销）
        self.dynamic.write().remove(name);
    }

    fn get(&self, name: &str) -> Option<Arc<dyn Tool>> {
        // 兼容现有 API：返回 Arc<dyn Tool>
        self.get_handle(name).map(|handle| handle.as_tool())
    }

    fn list(&self) -> Vec<Arc<dyn Tool>> {
        // 兼容现有 API：返回 Arc<dyn Tool> 列表
        self.list_handles()
            .into_iter()
            .map(|handle| handle.as_tool())
            .collect()
    }
}

// ── 测试 ──────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::{ToolContext, ToolResult};
    use async_trait::async_trait;
    use serde_json::{json, Value};

    // 测试用 Mock 工具
    struct MockTool {
        name: &'static str,
    }

    #[async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str {
            self.name
        }

        fn description(&self) -> &str {
            "mock tool"
        }

        fn input_schema(&self) -> Value {
            json!({ "type": "object" })
        }

        async fn execute(&self, _input: Value, _ctx: &ToolContext) -> ToolResult {
            ToolResult::ok_text(format!("executed: {}", self.name))
        }
    }

    #[test]
    fn test_hybrid_registry_builtin_priority() {
        let registry = HybridToolRegistry::new().with_builtin_tools(vec![(
            "bash",
            Arc::new(MockTool { name: "bash" }) as Arc<dyn Tool>,
        )]);

        // 验证内置工具可查找
        let handle = registry.get_handle("bash");
        assert!(handle.is_some());
        assert_eq!(handle.unwrap().name(), "bash");

        // 验证 ToolRegistry trait 兼容性
        let tool = registry.get("bash");
        assert!(tool.is_some());
        assert_eq!(tool.unwrap().name(), "bash");
    }

    #[test]
    fn test_hybrid_registry_dynamic_registration() {
        let registry = HybridToolRegistry::new();

        // 动态注册工具
        registry.register_arc(Arc::new(MockTool { name: "custom" }));

        // 验证动态工具可查找
        let handle = registry.get_handle("custom");
        assert!(handle.is_some());
        assert_eq!(handle.unwrap().name(), "custom");
    }

    #[test]
    fn test_hybrid_registry_builtin_overrides_dynamic() {
        let registry = HybridToolRegistry::new().with_builtin_tools(vec![(
            "bash",
            Arc::new(MockTool { name: "bash_builtin" }) as Arc<dyn Tool>,
        )]);

        // 尝试动态注册同名工具
        registry.register_arc(Arc::new(MockTool { name: "bash_dynamic" }));

        // 验证内置工具优先
        let handle = registry.get_handle("bash").unwrap();
        assert_eq!(handle.name(), "bash_builtin");
    }

    #[test]
    fn test_hybrid_registry_list() {
        let registry = HybridToolRegistry::new().with_builtin_tools(vec![
            (
                "bash",
                Arc::new(MockTool { name: "bash" }) as Arc<dyn Tool>,
            ),
            ("read", Arc::new(MockTool { name: "read" })),
        ]);

        registry.register_arc(Arc::new(MockTool { name: "custom" }));

        let handles = registry.list_handles();
        assert_eq!(handles.len(), 3);

        let names: Vec<&str> = handles.iter().map(|h| h.name()).collect();
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

        // 注销动态工具成功
        registry.unregister("custom");
        assert!(registry.get_handle("custom").is_none());

        // 注销内置工具无效（内置工具不可变）
        registry.unregister("bash");
        assert!(registry.get_handle("bash").is_some());
    }
}
