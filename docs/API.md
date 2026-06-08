# HyperAgent API Reference

## Web API (hyper serve)

The Web API uses Server-Sent Events (SSE) for streaming responses.

### POST /api/chat

Send a message to the agent. Returns SSE stream of tokens.

**Request:**
```json
{
  "message": "Your prompt here",
  "session_id": "optional-session-uuid"
}
```

**Response (SSE):**
```
data: I'll
data:  help
data:  you
data:  with
data:  that
data: [DONE]
```

**Session persistence:** Sessions are stored in memory. Reuse `session_id` to continue a conversation.

### GET /api/health

Check server status.

```json
{
  "status": "ok",
  "version": "0.2.0",
  "name": "HyperAgent"
}
```

### GET /api/sessions

List active sessions.

```json
{
  "count": 3,
  "sessions": ["uuid-1", "uuid-2", "uuid-3"]
}
```

### GET /api/share/:token

Resolve a shared session token.

```json
{
  "session_id": "...",
  "summary": "Session summary line",
  "messages": [{"role": "user", "content": "..."}],
  "token": "uuid-token"
}
```

## CLI API

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `HYPER_LLM_API_KEY` | LLM provider API key | (required) |
| `HYPER_LLM_MODEL` | Model name | `gpt-4o` |
| `HYPER_LLM_BASE_URL` | API base URL | `https://api.openai.com/v1` |
| `HYPER_LANG` | UI language (`en` or `zh-CN`) | Auto-detect from `LANG` |
| `HYPER_LLM_MAX_RETRIES` | Max retry attempts | 3 |
| `HYPER_LLM_TIMEOUT` | Request timeout (seconds) | 60 |
| `RUST_LOG` | Log level | `info` |

### Config File

Location: `~/.config/hyper/config.toml` (Linux) or `~/Library/Application Support/hyper/config.toml` (macOS)

```toml
[llm]
providers = [
  { model = "gpt-4o", base_url = "https://api.openai.com/v1", api_key = "${HYPER_LLM_API_KEY}" },
]

[sandbox]
enabled = true
image = "hyperagent-sandbox:latest"

[memory]
max_entries = 10000
auto_prune_threshold = 5000
auto_prune_below = 0.1
global_promote_threshold = 0.7

[git]
auto_commit = true
commit_style = "conventional"
```
