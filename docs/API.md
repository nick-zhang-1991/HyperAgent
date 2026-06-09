# HyperAgent API Reference

## Authentication

All endpoints require no auth — the server runs locally on `127.0.0.1`.

## Endpoints

### POST /api/chat — Chat with the Agent

Streaming SSE response. Tokens arrive in real-time.

```bash
curl -N -X POST http://127.0.0.1:3000/api/chat \
  -H "Content-Type: application/json" \
  -d '{"message": "explain Rust ownership"}'
```

Response (SSE):
```
data: Rust
data:  ownership
data:  is
data:  a
data:  system...
data: [DONE]
```

---

### POST /api/analyze — Code Analysis

```bash
curl -X POST http://127.0.0.1:3000/api/analyze
```

Response:
```json
{
  "ok": true,
  "total": 12,
  "critical": 2,
  "issues": [
    {"severity": "Critical", "file": "src/auth.rs", "line": 42, "message": "unwrap() in production path"}
  ]
}
```

---

### POST /api/eval — Self-Evaluation

```bash
curl -X POST http://127.0.0.1:3000/api/eval
```

Response:
```json
{"ok": true, "total": 4, "passed": 3, "fail_rate": 75.0}
```

---

### GET /api/memory/global — Global Knowledge

```bash
curl http://127.0.0.1:3000/api/memory/global
```

Response:
```json
{
  "ok": true,
  "count": 5,
  "memories": [
    {"content": "User prefers Rust for systems code", "importance": 0.92, "score": 0.85}
  ]
}
```

---

### POST /api/feedback — Train the Agent

```bash
# Positive feedback
curl -X POST http://127.0.0.1:3000/api/feedback \
  -d '{"kind":"good","reason":"followed Rust conventions"}'

# Correction
curl -X POST http://127.0.0.1:3000/api/feedback \
  -d '{"kind":"bad","reason":"used unwrap() instead of ?"}'
```

---

### GET /api/health — Health Check

```bash
curl http://127.0.0.1:3000/api/health
```
```json
{"status":"ok","version":"0.2.0","name":"HyperAgent"}
```

### GET /api/sessions — List Sessions

```bash
curl http://127.0.0.1:3000/api/sessions
```
```json
{"count":3,"sessions":["uuid-1","uuid-2","uuid-3"]}
```

### GET /api/share/:token — Resolve Shared Session

```bash
curl http://127.0.0.1:3000/api/share/abc123-def456
```
```json
{"session_id":"...","summary":"API design session","token":"abc123-def456"}
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `HYPER_LLM_API_KEY` | (required) | LLM API key |
| `HYPER_LLM_MODEL` | `gpt-4o` | Model name |
| `HYPER_LLM_BASE_URL` | `https://api.openai.com/v1` | API base URL |
| `HYPER_LANG` | Auto-detect | UI language (en, zh-CN, ja, etc.) |
