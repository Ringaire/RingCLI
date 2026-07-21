# linkkit-parser

Linkkit 协议解析器 - 支持 XML 和 JSON 两种格式的工具调用协议。

## 特性

- ✅ **XML 格式解析**：完整支持所有 Linkkit 标签
- ✅ **JSON 格式解析**：简化的 `doc` / `use` 调用格式
- ✅ **输出生成器**：统一的 `<output>` 标签生成
- ✅ **权限闸门**：read-gate 和 doc-gate 机制
- ✅ **类型安全**：强类型的 Rust API

## 快速开始

### XML 格式

```rust
use linkkit_parser::{LinkkitParser, LinkkitTag};

let xml = r#"
    <doc-read name="bash"/>
    <tool-use name="bash">ls -la</tool-use>
"#;

let mut parser = LinkkitParser::new(xml);
let tags = parser.parse()?;

for tag in tags {
    match tag {
        LinkkitTag::DocRead { name, .. } => {
            println!("读取文档: {:?}", name);
        }
        LinkkitTag::ToolUse { name, args } => {
            println!("调用工具: {}, 参数: {:?}", name, args);
        }
        _ => {}
    }
}
```

### JSON 格式

```rust
use linkkit_parser::{LinkkitJson, LinkkitTag};

// 读取文档
let json = r#"{"doc": "bash", "line": "1-50"}"#;
let cmd = LinkkitJson::parse(json)?;
let tag = cmd.into_tag()?;

// 调用工具
let json = r#"{"use": "bash", "meta": {"command": "ls -la"}}"#;
let cmd = LinkkitJson::parse(json)?;
let tag = cmd.into_tag()?;
```

### 输出生成

```rust
use linkkit_parser::{OutputGenerator, OutputLevel};

// 正常输出
let xml = OutputGenerator::xml(OutputLevel::Normal, None, "Task completed");
// <output>Task completed</output>

// 错误输出
let xml = OutputGenerator::xml(OutputLevel::Error, Some("bash"), "command not found");
// <output level="error" from="bash">command not found</output>

// JSON 格式
let json = OutputGenerator::json(OutputLevel::Done, Some("bash"), "Success");
// {"level":"done","from":"bash","content":"Success"}

// 纯文本（日志）
let text = OutputGenerator::text(OutputLevel::Warn, Some("system"), "Low memory");
// [WARN|system] Low memory
```

## 支持的标签

### 文档管理
- `<doc-ls/>` - 列出所有文档
- `<doc-read name="..."/>` - 读取指定文档

### 工具管理
- `<tool-ls/>` - 列出所有工具
- `<tool-info name="..."/>` - 查看工具详情
- `<tool-use name="...">...</tool-use>` - 调用工具
- `<tool-reload/>` - 重新加载工具

### 命令执行
- `<bash>...</bash>` - 执行 bash 命令
- `<bash-ls/>` - 列出后台任务
- `<bash-kill>...</bash-kill>` - 终止任务
- `<bash-log>...</bash-log>` - 查看任务日志

### 文件操作
- `<read file="..."/>` - 读取文件
- `<edit file="...">...</edit>` - 编辑文件
- `<write file="...">...</write>` - 写入文件
- `<tree path="..."/>` - 显示目录树

### 其他
- `<web-fetch>...</web-fetch>` - 抓取网页
- `<todo-update>...</todo-update>` - 更新待办
- `<sub-agent>...</sub-agent>` - 派生子 Agent
- `<event from="..."/>` - 事件订阅
- `<ask>...</ask>` - 询问用户

## JSON 格式对照

| XML | JSON |
|-----|------|
| `<doc-read name="bash"/>` | `{"doc": "bash"}` |
| `<doc-read name="bash" line="1-50"/>` | `{"doc": "bash", "line": "1-50"}` |
| `<tool-use name="bash">ls</tool-use>` | `{"use": "bash", "meta": "ls"}` |
| `<tool-use name="bash" timeout="10s">ls</tool-use>` | `{"use": "bash", "meta": {"command": "ls", "timeout": "10s"}}` |

## 输出级别

- `Normal` - 正常输出（默认）
- `Done` - 成功完成
- `Tip` - 提示信息
- `Warn` - 警告
- `Error` - 错误

## 许可证

MIT License
