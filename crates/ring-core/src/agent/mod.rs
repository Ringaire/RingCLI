// Agent 模型角色分类与 role-based 选模。
//
// 对齐 ringcode-bun 的 model-selector：把模型按能力分为 heavy/balanced/light/coding，
// 供 orchestrator 的 spawn_agent 按 role 自动挑选合适的子 agent 模型。
//
// 注：原版用了带负向先行断言 (?!...) 的正则，Rust 的 regex crate 不支持先行断言，
// 故此处用等价的显式字符串判断重写，行为一致且无额外依赖。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelRole {
    /// 复杂推理、多步规划、架构决策
    Heavy,
    /// 通用编码与分析（默认）
    Balanced,
    /// 快速廉价：简单编辑、查找、格式化
    Light,
    /// 代码专精模型
    Coding,
}

impl ModelRole {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Heavy => "heavy",
            Self::Balanced => "balanced",
            Self::Light => "light",
            Self::Coding => "coding",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            Self::Heavy    => "complex reasoning, multi-step planning, architecture decisions",
            Self::Balanced => "general-purpose coding and analysis",
            Self::Light    => "fast lookups, simple edits, formatting, quick summarization",
            Self::Coding   => "code generation, refactoring, code explanation",
        }
    }

    /// 全部角色（固定顺序，用于展示）。
    pub fn all() -> [ModelRole; 4] {
        [ModelRole::Heavy, ModelRole::Balanced, ModelRole::Light, ModelRole::Coding]
    }
}

impl std::str::FromStr for ModelRole {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "heavy"    => Ok(Self::Heavy),
            "balanced" => Ok(Self::Balanced),
            "light"    => Ok(Self::Light),
            "coding"   => Ok(Self::Coding),
            other      => Err(format!("unknown model role: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelCatalogEntry {
    pub id:   String,
    pub role: ModelRole,
}

// ── 分类 ──────────────────────────────────────────────────────────────────────

/// 把模型 id 分类到能力角色。
pub fn classify_model(model_id: &str) -> ModelRole {
    let id = model_id.to_lowercase();

    if is_coding(&id) {
        return ModelRole::Coding;
    }
    if is_heavy(&id) {
        return ModelRole::Heavy;
    }
    if is_light(&id) {
        return ModelRole::Light;
    }
    ModelRole::Balanced
}

fn is_coding(id: &str) -> bool {
    const CODING: &[&str] = &["codestral", "deepseek-coder", "starcoder", "codegemma"];
    CODING.iter().any(|k| id.contains(k))
}

fn is_heavy(id: &str) -> bool {
    // opus
    if id.contains("opus") {
        return true;
    }
    // o1- / o3-（OpenAI 推理模型）
    if id.contains("o1-") || id.contains("o3-") {
        return true;
    }
    // gpt-4，但排除 gpt-4o-mini
    if id.contains("gpt-4") && !id.contains("gpt-4o-mini") {
        return true;
    }
    // gemini pro / ultra
    if id.contains("gemini") && (id.contains("pro") || id.contains("ultra")) {
        return true;
    }
    // deepseek-r1 / r2
    if id.contains("deepseek-r1") || id.contains("deepseek-r2") {
        return true;
    }
    // 大号 llama：70/72/90/110B
    if id.contains("llama")
        && (id.contains("70b") || id.contains("72b") || id.contains("90b") || id.contains("110b"))
    {
        return true;
    }
    // qwen max
    if id.contains("qwen") && id.contains("max") {
        return true;
    }
    false
}

fn is_light(id: &str) -> bool {
    if id.contains("haiku") {
        return true;
    }
    if id.contains("gpt-3.5") {
        return true;
    }
    // gemini flash lite
    if id.contains("flash-lite") || id.contains("flash.lite") {
        return true;
    }
    // 小号 llama：3B–8B（个位数 + b）
    if id.contains("llama") && has_small_llama_size(id) {
        return true;
    }
    // phi-2 / phi-3 / phi-4
    if id.contains("phi-2") || id.contains("phi-3") || id.contains("phi-4") {
        return true;
    }
    // mistral-7b
    if id.contains("mistral-7b") {
        return true;
    }
    // qwen 1.5b
    if id.contains("qwen") && id.contains("1.5b") {
        return true;
    }
    false
}

/// 检测形如 "...3b".."...8b" 的小模型尺寸（个位数 3-8 紧跟 b，且 b 后不接数字）。
fn has_small_llama_size(id: &str) -> bool {
    let bytes = id.as_bytes();
    for i in 0..bytes.len() {
        let c = bytes[i];
        if (b'3'..=b'8').contains(&c) {
            // 前一个字符不能是数字（避免 70b 里的 0b 之类误判，这里要求 3-8 是独立尺寸位）
            let prev_is_digit = i > 0 && bytes[i - 1].is_ascii_digit();
            if prev_is_digit {
                continue;
            }
            // 紧跟 'b'
            if i + 1 < bytes.len() && (bytes[i + 1] == b'b') {
                // 'b' 后不能紧跟数字
                let after_b_is_digit = i + 2 < bytes.len() && bytes[i + 2].is_ascii_digit();
                if !after_b_is_digit {
                    return true;
                }
            }
        }
    }
    false
}

// ── 选模 ──────────────────────────────────────────────────────────────────────

/// 角色回退链：找不到精确角色时按链顺序退化。
fn role_fallbacks(role: ModelRole) -> [ModelRole; 4] {
    use ModelRole::*;
    match role {
        Heavy    => [Heavy, Balanced, Coding, Light],
        Balanced => [Balanced, Heavy, Light, Coding],
        Light    => [Light, Balanced, Coding, Heavy],
        Coding   => [Coding, Balanced, Heavy, Light],
    }
}

/// 按角色从目录中选一个模型 id；都没有则返回 fallback。
pub fn select_model_by_role(role: ModelRole, catalog: &[ModelCatalogEntry], fallback: &str) -> String {
    for r in role_fallbacks(role) {
        if let Some(m) = catalog.iter().find(|m| m.role == r) {
            return m.id.clone();
        }
    }
    fallback.to_string()
}

/// 从模型 id 列表构建目录（去重，保序）。
pub fn build_model_catalog(model_ids: &[String]) -> Vec<ModelCatalogEntry> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for id in model_ids {
        if seen.insert(id.clone()) {
            out.push(ModelCatalogEntry { id: id.clone(), role: classify_model(id) });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify() {
        assert_eq!(classify_model("claude-opus-4-5"), ModelRole::Heavy);
        assert_eq!(classify_model("claude-haiku-4-5"), ModelRole::Light);
        assert_eq!(classify_model("gpt-4o"), ModelRole::Heavy);
        assert_eq!(classify_model("gpt-4o-mini"), ModelRole::Balanced);
        assert_eq!(classify_model("gpt-3.5-turbo"), ModelRole::Light);
        assert_eq!(classify_model("deepseek-coder-v2"), ModelRole::Coding);
        assert_eq!(classify_model("deepseek-chat"), ModelRole::Balanced);
        assert_eq!(classify_model("deepseek-r1"), ModelRole::Heavy);
        assert_eq!(classify_model("gemini-2.5-pro"), ModelRole::Heavy);
        assert_eq!(classify_model("gemini-2.0-flash"), ModelRole::Balanced);
        assert_eq!(classify_model("llama-3.3-70b-versatile"), ModelRole::Heavy);
        assert_eq!(classify_model("mistral-7b"), ModelRole::Light);
        assert_eq!(classify_model("codestral-latest"), ModelRole::Coding);
    }

    #[test]
    fn select_with_fallback() {
        let catalog = vec![
            ModelCatalogEntry { id: "claude-opus-4-5".into(),  role: ModelRole::Heavy },
            ModelCatalogEntry { id: "claude-haiku-4-5".into(), role: ModelRole::Light },
        ];
        assert_eq!(select_model_by_role(ModelRole::Heavy, &catalog, "fb"), "claude-opus-4-5");
        assert_eq!(select_model_by_role(ModelRole::Light, &catalog, "fb"), "claude-haiku-4-5");
        // coding 不存在 → 回退到 balanced（无）→ heavy（有）
        assert_eq!(select_model_by_role(ModelRole::Coding, &catalog, "fb"), "claude-opus-4-5");
        // 空目录 → fallback
        assert_eq!(select_model_by_role(ModelRole::Heavy, &[], "fb"), "fb");
    }
}
