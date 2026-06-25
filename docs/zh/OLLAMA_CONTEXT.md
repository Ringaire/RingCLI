# Ollama 大上下文配置指南

## 方法一：创建自定义 Modelfile（推荐）

这是**最可靠**的方式，直接修改模型本身的上下文窗口限制。

### 1. 拉取基础模型

```bash
ollama pull qwen3:8b
```

### 2. 创建 Modelfile

```bash
mkdir -p ~/.ollama/models
cat > ~/.ollama/models/Modelfile.qwen3-context << 'EOF'
FROM qwen3:8b

# 设置上下文窗口为 128k（根据模型能力调整）
PARAMETER num_ctx 131072
EOF
```

### 3. 创建新模型

```bash
ollama create qwen3-128k -f ~/.ollama/models/Modelfile.qwen3-context
```

### 4. 验证

```bash
ollama run qwen3-128k "Hello"
```

### 5. 在 NekoCLI 中使用

在 `~/.config/neko/settings.jsonc` 中配置：

```jsonc
{
  "model": "ollama/qwen3-128k"
}
```

---

## 方法二：运行时参数（临时）

某些模型支持在请求时动态调整 `num_ctx`，但这取决于模型是否支持。

在 `~/.config/neko/providers.json` 中配置：

```json
{
  "ollama": {
    "name": "Ollama",
    "type": "openai-compatible",
    "base_url": "http://localhost:11434/v1",
    "api_key_env": null,
    "default_model": "qwen3-128k",
    "extra_body": {
      "options": {
        "num_ctx": 131072
      }
    }
  }
}
```

> ⚠️ **注意**：如果模型本身的 `num_ctx` 限制较小（如默认 8k 或 32k），即使请求时传入更大的值也不会生效。**方法一才是根本解决方案**。

---

## 常见模型的上下文限制

| 模型 | 默认 num_ctx | 最大支持 |
|------|-------------|---------|
| llama3.2 | 8,192 | 131,072 |
| llama3.1 | 8,192 | 131,072 |
| qwen3 | 32,768 | 262,144 |
| qwen2.5 | 32,768 | 131,072 |
| mistral | 8,192 | 32,768 |
| mixtral | 32,768 | 65,536 |
| gemma2 | 8,192 | 32,768 |

---

## 验证当前模型的 num_ctx

```bash
# 查看模型信息
ollama show qwen3:8b

# 或查看运行中的配置
curl http://localhost:11434/api/tags | jq
```

---

## 故障排除

### 问题：设置后仍然被截断

**原因**：模型本身的 `num_ctx` 限制未达到。

**解决**：使用方法一重新创建模型，确保 `PARAMETER num_ctx` 足够大。

### 问题：内存不足 OOM

**原因**：上下文窗口越大，显存占用越高。

**估算公式**：
- 7B 模型，128k 上下文 ≈ 需要 32GB+ 显存
- 7B 模型，32k 上下文 ≈ 需要 16GB 显存
- 7B 模型，8k 上下文 ≈ 需要 8GB 显存

**解决**：降低 `num_ctx` 或使用更小的模型。

### 问题：NekoCLI 工具调用中断

确保使用的是支持工具调用的模型（如 qwen3、llama3.1+），旧模型可能不支持。