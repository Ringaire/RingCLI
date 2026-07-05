use criterion::{black_box, criterion_group, criterion_main, Criterion};
use neko_core::tools::{
    DefaultToolRegistry, HybridToolRegistry, Tool, ToolContext, ToolRegistry, ToolResult,
};
use std::sync::Arc;

// ── Mock 工具实现 ─────────────────────────────────────────────────────────────

struct BashTool;

#[async_trait::async_trait]
impl Tool for BashTool {
    fn name(&self) -> &str {
        "bash"
    }

    fn description(&self) -> &str {
        "Execute bash commands"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "command": { "type": "string" }
            }
        })
    }

    async fn execute(&self, _input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        ToolResult::ok_text("executed")
    }
}

struct ReadFileTool;

#[async_trait::async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn description(&self) -> &str {
        "Read file content"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" }
            }
        })
    }

    async fn execute(&self, _input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        ToolResult::ok_text("file content")
    }
}

struct WriteFileTool;

#[async_trait::async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn description(&self) -> &str {
        "Write file content"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "path": { "type": "string" },
                "content": { "type": "string" }
            }
        })
    }

    async fn execute(&self, _input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        ToolResult::ok_text("written")
    }
}

struct GrepTool;

#[async_trait::async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn description(&self) -> &str {
        "Search for patterns"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string" }
            }
        })
    }

    async fn execute(&self, _input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        ToolResult::ok_text("matches")
    }
}

struct GlobTool;

#[async_trait::async_trait]
impl Tool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn description(&self) -> &str {
        "File pattern matching"
    }

    fn input_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string" }
            }
        })
    }

    async fn execute(&self, _input: serde_json::Value, _ctx: &ToolContext) -> ToolResult {
        ToolResult::ok_text("files")
    }
}

// ── 基准测试：HybridToolRegistry 查找性能 ─────────────────────────────────────

fn bench_hybrid_registry_lookup_builtin(c: &mut Criterion) {
    let registry = HybridToolRegistry::new().with_builtin_tools(vec![
        ("bash", Arc::new(BashTool) as Arc<dyn Tool>),
        ("read_file", Arc::new(ReadFileTool)),
        ("write_file", Arc::new(WriteFileTool)),
        ("grep", Arc::new(GrepTool)),
        ("glob", Arc::new(GlobTool)),
    ]);

    c.bench_function("hybrid_registry_lookup_builtin", |b| {
        b.iter(|| {
            black_box(registry.get("bash"));
        });
    });
}

fn bench_hybrid_registry_lookup_dynamic(c: &mut Criterion) {
    let registry = HybridToolRegistry::new();
    registry.register_arc(Arc::new(BashTool));
    registry.register_arc(Arc::new(ReadFileTool));
    registry.register_arc(Arc::new(WriteFileTool));
    registry.register_arc(Arc::new(GrepTool));
    registry.register_arc(Arc::new(GlobTool));

    c.bench_function("hybrid_registry_lookup_dynamic", |b| {
        b.iter(|| {
            black_box(registry.get("bash"));
        });
    });
}

fn bench_hybrid_registry_list(c: &mut Criterion) {
    let registry = HybridToolRegistry::new().with_builtin_tools(vec![
        ("bash", Arc::new(BashTool) as Arc<dyn Tool>),
        ("read_file", Arc::new(ReadFileTool)),
        ("write_file", Arc::new(WriteFileTool)),
    ]);
    registry.register_arc(Arc::new(GrepTool));
    registry.register_arc(Arc::new(GlobTool));

    c.bench_function("hybrid_registry_list", |b| {
        b.iter(|| {
            black_box(registry.list());
        });
    });
}

// ── 基准测试：DefaultToolRegistry 查找性能 ───────────────────────────────────

fn bench_default_registry_lookup(c: &mut Criterion) {
    let registry = DefaultToolRegistry::new();
    registry.register_arc(Arc::new(BashTool));
    registry.register_arc(Arc::new(ReadFileTool));
    registry.register_arc(Arc::new(WriteFileTool));
    registry.register_arc(Arc::new(GrepTool));
    registry.register_arc(Arc::new(GlobTool));

    c.bench_function("default_registry_lookup", |b| {
        b.iter(|| {
            black_box(registry.get("bash"));
        });
    });
}

fn bench_default_registry_list(c: &mut Criterion) {
    let registry = DefaultToolRegistry::new();
    registry.register_arc(Arc::new(BashTool));
    registry.register_arc(Arc::new(ReadFileTool));
    registry.register_arc(Arc::new(WriteFileTool));
    registry.register_arc(Arc::new(GrepTool));
    registry.register_arc(Arc::new(GlobTool));

    c.bench_function("default_registry_list", |b| {
        b.iter(|| {
            black_box(registry.list());
        });
    });
}

// ── 基准测试：注册性能 ───────────────────────────────────────────────────────

fn bench_hybrid_registry_register(c: &mut Criterion) {
    c.bench_function("hybrid_registry_register", |b| {
        b.iter(|| {
            let registry = HybridToolRegistry::new();
            registry.register_arc(Arc::new(BashTool));
            black_box(registry);
        });
    });
}

fn bench_default_registry_register(c: &mut Criterion) {
    c.bench_function("default_registry_register", |b| {
        b.iter(|| {
            let registry = DefaultToolRegistry::new();
            registry.register_arc(Arc::new(BashTool));
            black_box(registry);
        });
    });
}

// ── 基准测试组 ───────────────────────────────────────────────────────────────

criterion_group!(
    lookup_benches,
    bench_hybrid_registry_lookup_builtin,
    bench_hybrid_registry_lookup_dynamic,
    bench_default_registry_lookup,
);

criterion_group!(
    list_benches,
    bench_hybrid_registry_list,
    bench_default_registry_list,
);

criterion_group!(
    register_benches,
    bench_hybrid_registry_register,
    bench_default_registry_register,
);

criterion_main!(lookup_benches, list_benches, register_benches);
