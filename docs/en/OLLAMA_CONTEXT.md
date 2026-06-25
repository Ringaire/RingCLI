# Ollama Large Context Configuration Guide

## Method 1: Create Custom Modelfile (Recommended)

This is the **most reliable** way to directly modify the model's context window limit.

### 1. Pull Base Model

```bash
ollama pull qwen3:8b
```

### 2. Create Modelfile

```bash
mkdir -p ~/.ollama/models
cat > ~/.ollama/models/Modelfile.qwen3-context << 'EOF'
FROM qwen3:8b

# Set context window to 128k (adjust based on model capability)
PARAMETER num_ctx 131072
EOF
```

### 3. Create New Model

```bash
ollama create qwen3-128k -f ~/.ollama/models/Modelfile.qwen3-context
```

### 4. Verify

```bash
ollama run qwen3-128k "Hello"
```

### 5. Use in NekoCLI

Configure in `~/.config/neko/settings.jsonc`:

```jsonc
{
  "model": "ollama/qwen3-128k"
}
```

---

## Method 2: Runtime Parameters (Temporary)

Some models support dynamically adjusting `num_ctx` at request time, but this depends on model support.

Configure in `~/.config/neko/providers.json`:

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

> ⚠️ **Note**: If the model's built-in `num_ctx` limit is small (e.g., default 8k or 32k), passing a larger value at request time won't work. **Method 1 is the fundamental solution**.

---

## Common Model Context Limits

| Model | Default num_ctx | Max Supported |
|-------|-----------------|---------------|
| llama3.2 | 8,192 | 131,072 |
| llama3.1 | 8,192 | 131,072 |
| qwen3 | 32,768 | 262,144 |
| qwen2.5 | 32,768 | 131,072 |
| mistral | 8,192 | 32,768 |
| mixtral | 32,768 | 65,536 |
| gemma2 | 8,192 | 32,768 |

---

## Verify Current Model num_ctx

```bash
# View model info
ollama show qwen3:8b

# Or view running config
curl http://localhost:11434/api/tags | jq
```

---

## Troubleshooting

### Issue: Still truncated after setting

**Cause**: Model's built-in `num_ctx` limit not reached.

**Solution**: Use Method 1 to recreate model, ensure `PARAMETER num_ctx` is large enough.

### Issue: Out of Memory (OOM)

**Cause**: Larger context window = higher VRAM usage.

**Estimation Formula**:
- 7B model, 128k context ≈ 32GB+ VRAM required
- 7B model, 32k context ≈ 16GB VRAM required
- 7B model, 8k context ≈ 8GB VRAM required

**Solution**: Reduce `num_ctx` or use a smaller model.

### Issue: NekoCLI Tool Calls Interrupting

Ensure you're using a model that supports tool calls (e.g., qwen3, llama3.1+). Older models may not support this feature.