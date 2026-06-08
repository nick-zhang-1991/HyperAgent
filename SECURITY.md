# Security Policy

## Reporting Vulnerabilities

If you discover a security vulnerability, please DO NOT open a public issue.

Email: security@hyperagent.dev (or open a private security advisory on GitHub)

## Supported Versions

| Version | Supported |
|---------|-----------|
| v0.2.0  | ✅ Active |
| v0.1.0  | ❌ EOL |

## Security Features

HyperAgent includes built-in security measures:

- **Tool safety gates** — Commands are classified by danger level (Low/Medium/High/Critical) with guard checks
- **Docker sandbox** — High-risk operations run in isolated containers
- **No secrets in logs** — Token/key values are masked in output
- **Provider isolation** — Each LLM provider has independent API key storage

## Best Practices

1. Never commit `.env` files or API keys
2. Use `HYPER_LLM_API_KEY` environment variable instead of config files
3. Run `hyper analyze` regularly to check for dependency vulnerabilities
4. Keep HyperAgent updated (`cargo install --force hyperagent`)
