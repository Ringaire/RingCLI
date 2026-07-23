//! 图像分析工具：将图片转发给视觉辅助模型（vision_model）识别。
//!
//! 用途：主模型不支持 image（如 DeepSeek-R1）时，可调用此工具，
//! 把图片转发给配置的视觉辅助模型（如 GLM-4V）识别文字或描述图片。
//!
//! 配置：settings.jsonc 的 `vision_model` 字段（provider/model 格式）。
//! 未配置时工具返回错误提示。

use async_trait::async_trait;
use ring_core::tools::{ContentBlock, Message, MessageRole, Tool, ToolContext, ToolResult};
use ring_core::image;
use ring_providers::provider::{vision_provider, ChatRequest};
use serde_json::{json, Value};
use std::path::Path;
use tokio_util::sync::CancellationToken;
use tracing::warn;

pub struct ImageAnalyzeTool;

#[async_trait]
impl Tool for ImageAnalyzeTool {
    fn name(&self) -> &str {
        "image_analyze"
    }

    fn description(&self) -> &str {
        "Analyze an image using a vision model. Use this when the current model does not support \
         images natively, or when you need OCR/text extraction / image description. \
         Forwards the image to the configured vision_model (e.g. GLM-4V)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "image": {
                    "type": "string",
                    "description": "Image file path (absolute or relative to cwd), OR a base64 data URI (data:image/...;base64,...)"
                },
                "question": {
                    "type": "string",
                    "description": "What to analyze: e.g. 'describe this image', 'extract all text (OCR)', 'what objects are visible'. Default: describe.",
                    "default": "Describe this image in detail."
                }
            },
            "required": ["image"]
        })
    }

    fn prompt(&self) -> Option<&str> {
        Some(
            "Use `image_analyze` to process images when you cannot see them directly. \
             It forwards the image to a vision-capable model and returns text. \
             Common uses: OCR (extract text), image description, object detection, \
             reading charts/diagrams/screenshots.",
        )
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> ToolResult {
        let image_arg = input
            .get("image")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let question = input
            .get("question")
            .and_then(|v| v.as_str())
            .unwrap_or("Describe this image in detail.");

        if image_arg.is_empty() {
            return ToolResult::err("missing required parameter: image (path or data URI)");
        }

        // 解析图片来源：路径 or data URI
        let (media_type, data) = if let Some(b64) = image_arg.strip_prefix("data:") {
            // data URI: data:image/jpeg;base64,xxxx
            parse_data_uri(b64)
        } else {
            // 文件路径：读取 + 压缩
            let path = if Path::new(image_arg).is_absolute() {
                Path::new(image_arg).to_path_buf()
            } else {
                ctx.cwd.join(image_arg)
            };
            if !path.is_file() {
                return ToolResult::err(format!("image file not found: {}", path.display()));
            }
            match image::read_and_compress(&path, image::DEFAULT_MAX_BYTES) {
                Some(x) => x,
                None => return ToolResult::err(format!("failed to read/compress image: {}", path.display())),
            }
        };

        // 取视觉辅助 provider
        let Some(vision) = vision_provider() else {
            return ToolResult::err(
                "No vision model configured. Set `vision_model` in settings.jsonc \
                 (e.g. \"zhipu/glm-4v-flash\") to enable image analysis.",
            );
        };

        // 构造请求：图片 + 问题
        let req = ChatRequest::new(
            vision.default_model().to_string(),
            vec![Message::new(
                MessageRole::User,
                vec![
                    ContentBlock::Text { text: question.to_string() },
                    ContentBlock::Image { media_type, data },
                ],
            )],
        );
        // vision 请求通常不需要 tools，max_tokens 用默认

        match vision.chat(&req, CancellationToken::new()).await {
            Ok(resp) => {
                // 提取 assistant 文本
                let text: String = resp
                    .message
                    .content
                    .iter()
                    .filter_map(|b| match b {
                        ContentBlock::Text { text } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n");
                if text.trim().is_empty() {
                    ToolResult::ok_text("[vision model returned empty response]")
                } else {
                    ToolResult::ok_text(text)
                }
            }
            Err(e) => {
                warn!(err = %e, "vision provider chat failed");
                ToolResult::err(format!("vision model request failed: {e}"))
            }
        }
    }
}

/// 解析 data URI（`data:` 后的部分）：image/jpeg;base64,xxxx → (mime, b64)
fn parse_data_uri(rest: &str) -> (String, String) {
    // rest 形如 "image/jpeg;base64,xxxx"
    if let Some((meta, b64)) = rest.split_once(',') {
        let mime = meta.split(';').next().unwrap_or("image/jpeg");
        return (mime.to_string(), b64.to_string());
    }
    ("image/jpeg".to_string(), rest.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_data_uri_jpeg() {
        let (m, d) = parse_data_uri("image/jpeg;base64,abc123");
        assert_eq!(m, "image/jpeg");
        assert_eq!(d, "abc123");
    }

    #[test]
    fn parse_data_uri_png() {
        let (m, d) = parse_data_uri("image/png;base64,zZZ=");
        assert_eq!(m, "image/png");
        assert_eq!(d, "zZZ=");
    }
}
