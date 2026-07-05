# linkkit-parser

Linkkit 协议解析器 - 为 nekocli 提供 XML 标签解析、权限闸门和命令执行支持。

## 功能特性

- **XML 解析**：基于 `quick-xml` 的高性能解析器
- **权限闸门**：实现 read-gate 和 doc-gate 两道权限检查
- **类型安全**：完整的 Rust 类型定义，零运行时错误
- **生产级代码**：完整的错误处理、单元测试覆盖

## 架构设计

### 核心模块

```
linkkit-parser/
├── error.rs      # 错误类型定义
├── tags.rs       # Linkkit 标签类型
├── parser.rs     # XML 解析器
├── gate.rs       # 权限闸门
└── executor.rs   # 命令执行器
```

### 关键设计模式

#### 1. 两道闸门（Gate Pattern）

```rust
// read-gate: 修改文件前必须先读取
gate.check_edit_gate(&file)?;

// doc-gate: 调用工具前必须先读取文档
gate.check_doc_gate(&tool_name)?;
```

#### 2. 标签类型系统

所有 Linkkit 标签都被建模为 Rust enum：

```rust
pub enum LinkkitTag {
    DocLs,
    DocRead { name: Option<String>, line: Option<String> },
    Bash { command: String, timeout: Option<u64>, ... },
    Read { file: PathBuf, line: Option<String>, ... },
    Edit { file: PathBuf, old: Option<String>, ... },
    ToolUse { name: String, args: ToolArgs },
    // ...
}
```

#### 3. 工具参数多态

```rust
pub enum ToolArgs {
    Single(String),           // 单参数
    Multiple(HashMap<String, String>),  // 多参数
}
```

## 使用示例

### 基本解析

```rust
use linkkit_parser::{LinkkitParser, LinkkitExecutor};

// 解析 XML
let input = r#"<doc-ls/>"#;
let mut parser = LinkkitParser::new(input);
let tags = parser.parse()?;

// 执行标签
let mut executor = LinkkitExecutor::new();
for tag in tags {
    let result = executor.execute(tag).await?;
    // 处理 result
}
```

### 闸门使用

```rust
use linkkit_parser::{LinkkitGate, GateError};
use std::path::PathBuf;

let mut gate = LinkkitGate::new();

// 标记文件已读
gate.mark_read(PathBuf::from("src/main.rs"));

// 检查编辑权限
match gate.check_edit_gate(&PathBuf::from("src/main.rs")) {
    Ok(()) => println!("允许编辑"),
    Err(GateError::MustReadFirst(path)) => {
        println!("必须先读取文件: {:?}", path);
    }
    _ => {}
}
```

### 完整工作流

```rust
use linkkit_parser::*;

async fn process_linkkit_input(input: &str) -> LinkkitResult<()> {
    // 1. 解析
    let mut parser = LinkkitParser::new(input);
    let tags = parser.parse()?;
    
    // 2. 执行
    let mut executor = LinkkitExecutor::new();
    for tag in tags {
        match executor.execute(tag).await? {
            ExecutionResult::Read { file, .. } => {
                // 调用 neko-tools 的 ReadTool
                println!("读取文件: {:?}", file);
            }
            ExecutionResult::ToolUse { name, args } => {
                // 调用相应工具
                println!("调用工具: {}", name);
            }
            _ => {}
        }
    }
    
    Ok(())
}
```

## 与 Claude Code 工具的对比

### 相同点

- 都有文件读写权限检查
- 都支持工具/技能的懒加载
- 都有完整的错误处理机制

### 不同点

| 特性 | Claude Code | Linkkit Parser |
|------|-------------|----------------|
| 协议格式 | JSON-based Tool Schema | XML DSL |
| 权限模型 | 单层权限检查 | 两道闸（read-gate + doc-gate） |
| 工具注册 | 运行时动态注册 | 编译时类型安全 |
| 文档管理 | Defer loading | 显式 doc-read |

### 从 Claude Code 借鉴的设计

1. **read-gate 模式**：
   - Claude Code: `readFileState.get(fullFilePath)`
   - Linkkit: `gate.check_edit_gate(&file)`

2. **参数验证**：
   - Claude Code: `validateInput()` 方法
   - Linkkit: Parser 内置验证 + 类型系统

3. **路径规范化**：
   - Claude Code: `expandPath()` 处理 `~` 和相对路径
   - Linkkit: 需要在上层工具实现中添加

## 集成到 nekocli

### 1. 在 neko-engine 中集成

```rust
// neko-engine/src/linkkit_mode.rs
use linkkit_parser::{LinkkitParser, LinkkitExecutor, ExecutionResult};

pub struct LinkkitMode {
    executor: LinkkitExecutor,
    tool_registry: Arc<dyn ToolRegistry>,
}

impl LinkkitMode {
    pub async fn process(&mut self, input: &str) -> Result<String> {
        let mut parser = LinkkitParser::new(input);
        let tags = parser.parse()?;
        
        for tag in tags {
            let result = self.executor.execute(tag).await?;
            self.dispatch_to_tool(result).await?;
        }
        
        Ok("完成".to_string())
    }
    
    async fn dispatch_to_tool(&self, result: ExecutionResult) -> Result<()> {
        match result {
            ExecutionResult::Bash { command, .. } => {
                self.tool_registry.get("bash")?.execute(/* ... */);
            }
            ExecutionResult::Read { file, .. } => {
                self.tool_registry.get("read")?.execute(/* ... */);
            }
            // ...
        }
        Ok(())
    }
}
```

### 2. 添加权限模式映射

```rust
// neko-core/src/permissions/mod.rs
impl ModeName {
    pub fn to_linkkit_mode(&self) -> &str {
        match self {
            ModeName::Ask => "ask",
            ModeName::Edit => "edit",
            ModeName::Build => "build",
            ModeName::Agent => "agent",
            _ => "build",
        }
    }
}
```

## 测试覆盖

- ✅ XML 解析（单标签、嵌套标签、属性解析）
- ✅ 权限闸门（read-gate、doc-gate）
- ✅ 执行器流程（标签执行、批量执行）
- ✅ 错误处理（解析错误、闸门错误）

运行测试：

```bash
cargo test --package linkkit-parser
```

## 性能特性

- **零拷贝解析**：使用 `quick-xml` 的流式解析
- **最小分配**：复用缓冲区，减少内存分配
- **类型安全**：编译期检查，无运行时类型转换开销

## 后续改进

1. **上下文传递**：为 `executor.execute()` 添加 `ToolContext` 参数
2. **工具注册表集成**：连接到 `neko-tools` 的工具实现
3. **完整的 Linkkit 协议支持**：
   - `<sub-agent>` 子 Agent 调度
   - `<event>` 事件订阅
   - `<tree>` 目录浏览
4. **错误恢复机制**：实现 Linkkit 协议的重试熔断阶梯

## 参考资料

- [Linkkit 协议规范](/d/Projects/Ringaire/.produce/linkkit-all/Linkkit 协议.md)
- [Open-ClaudeCode 工具实现](https://github.com/anthropics/anthropic-quickstarts)
- [quick-xml 文档](https://docs.rs/quick-xml/)
