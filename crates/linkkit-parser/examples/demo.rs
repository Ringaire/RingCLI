//! Linkkit 协议示例
//!
//! 演示 XML 和 JSON 两种格式的解析和输出生成。

use linkkit_parser::{
    LinkkitParser, LinkkitJson, LinkkitTag, ToolArgs,
    OutputGenerator, OutputLevel,
};

fn main() {
    println!("=== Linkkit Parser Examples ===\n");

    // ─── XML 格式示例 ───────────────────────────────────────────────────────
    println!("1. XML Format Parsing\n");
    
    let xml = r#"
        <doc-read name="bash" line="1-50"/>
        <tool-use name="bash">ls -la</tool-use>
        <bash timeout="10s">cargo build</bash>
        <read file="Cargo.toml" tail="20"/>
    "#;

    let mut parser = LinkkitParser::new(xml);
    match parser.parse() {
        Ok(tags) => {
            println!("Parsed {} tags from XML:\n", tags.len());
            for (i, tag) in tags.iter().enumerate() {
                println!("  [{}] {:?}", i + 1, tag);
            }
        }
        Err(e) => eprintln!("Parse error: {}", e),
    }

    println!("\n{}\n", "=".repeat(60));

    // ─── JSON 格式示例 ───────────────────────────────────────────────────────
    println!("2. JSON Format Parsing\n");

    // 示例 1: 读取文档
    let json1 = r#"{"doc": "bash", "line": "1-50", "say": "查看 bash 工具用法"}"#;
    println!("Input:  {}", json1);
    
    match LinkkitJson::parse(json1) {
        Ok(cmd) => {
            println!("Parsed: {:?}", cmd);
            match cmd.into_tag() {
                Ok(tag) => println!("Tag:    {:?}\n", tag),
                Err(e) => eprintln!("Convert error: {}\n", e),
            }
        }
        Err(e) => eprintln!("Parse error: {}\n", e),
    }

    // 示例 2: 调用工具（对象参数）
    let json2 = r#"{"use": "bash", "meta": {"command": "ls -la", "timeout": "10s"}}"#;
    println!("Input:  {}", json2);
    
    match LinkkitJson::parse(json2) {
        Ok(cmd) => {
            println!("Parsed: {:?}", cmd);
            match cmd.into_tag() {
                Ok(tag) => println!("Tag:    {:?}\n", tag),
                Err(e) => eprintln!("Convert error: {}\n", e),
            }
        }
        Err(e) => eprintln!("Parse error: {}\n", e),
    }

    // 示例 3: 调用工具（字符串参数）
    let json3 = r#"{"use": "bash", "meta": "cargo test"}"#;
    println!("Input:  {}", json3);
    
    match LinkkitJson::parse(json3) {
        Ok(cmd) => {
            println!("Parsed: {:?}", cmd);
            match cmd.into_tag() {
                Ok(tag) => println!("Tag:    {:?}\n", tag),
                Err(e) => eprintln!("Convert error: {}\n", e),
            }
        }
        Err(e) => eprintln!("Parse error: {}\n", e),
    }

    println!("{}\n", "=".repeat(60));

    // ─── 输出生成示例 ───────────────────────────────────────────────────────
    println!("3. Output Generation\n");

    // XML 格式输出
    println!("XML Outputs:");
    println!("  {}", OutputGenerator::xml(OutputLevel::Normal, None, "Task completed"));
    println!("  {}", OutputGenerator::xml(OutputLevel::Done, Some("bash"), "Command executed successfully"));
    println!("  {}", OutputGenerator::xml(OutputLevel::Warn, Some("system"), "Low disk space"));
    println!("  {}", OutputGenerator::xml(OutputLevel::Error, Some("bash"), "command not found"));

    println!("\nJSON Outputs:");
    println!("  {}", OutputGenerator::json(OutputLevel::Normal, None, "Task completed"));
    println!("  {}", OutputGenerator::json(OutputLevel::Done, Some("bash"), "Success"));
    println!("  {}", OutputGenerator::json(OutputLevel::Error, Some("tool"), "Invalid input"));

    println!("\nText Outputs (for logs):");
    println!("  {}", OutputGenerator::text(OutputLevel::Normal, None, "Started"));
    println!("  {}", OutputGenerator::text(OutputLevel::Done, Some("bash"), "Finished"));
    println!("  {}", OutputGenerator::text(OutputLevel::Error, Some("system"), "Critical failure"));

    println!("\n{}\n", "=".repeat(60));

    // ─── 标签类型判断示例 ───────────────────────────────────────────────────
    println!("4. Tag Type Checking\n");

    let tags_to_check = vec![
        LinkkitTag::DocRead { name: Some("bash".into()), line: None },
        LinkkitTag::ToolUse { name: "bash".into(), args: ToolArgs::Single("ls".into()) },
        LinkkitTag::Read { file: "test.txt".into(), line: None, tail: None },
        LinkkitTag::Bash { command: "echo hello".into(), timeout: None, tail: None, bg: false, at: None },
    ];

    for tag in tags_to_check {
        println!("  {:<20} -> read_only: {}", 
            tag.tag_name(), 
            tag.is_read_only()
        );
    }

    println!("\n=== End of Examples ===");
}
