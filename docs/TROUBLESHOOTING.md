# Troubleshooting Guide / 故障排查指南

## Common Issues / 常见问题

### 1. "hyper: command not found"

```bash
# Ensure ~/.local/bin is in PATH
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.bashrc
source ~/.bashrc

# Or reinstall
cargo install hyperagent
```

### 2. LLM Connection Error / LLM 连接失败

```bash
# Check API key
echo $HYPER_LLM_API_KEY

# Check if you can reach the API
curl -s -H "Authorization: Bearer $HYPER_LLM_API_KEY" \
  "$HYPER_LLM_BASE_URL/models" | head -5

# Try with verbose logging
RUST_LOG=debug hyper run "test" --mode ask
```

### 3. Build Hangs / 编译卡住

```bash
# Kill stale rustc processes
pkill -f rustc
sleep 5

# Retry with single job
cargo build -j 1
```

### 4. Memory DB Corruption / 记忆库损坏

```bash
# Reset memory (loses all stored knowledge)
rm ~/.local/share/hyperagent/memory.db
hyper init --yes   # Rebuild
```

### 5. Web UI Not Connecting / Web 界面连不上

```bash
# Start server
hyper serve

# Check if running
curl http://127.0.0.1:3000/api/health

# Start Web UI (separate terminal)
cd gui && pnpm dev
```

### 6. Windows Encoding Issues / Windows 中文乱码

```powershell
# Set UTF-8 encoding
chcp 65001
$env:HYPER_LANG="zh-CN"
```

### 7. Token Limit Exceeded / Token 溢出

- Use `--mode ask` for longer contexts
- Clear old sessions: `hyper session list` then delete
- Increase in config: `max_tokens = 100000`

### 8. Docker Sandbox Not Available / Docker 沙箱不可用

```bash
docker ps   # Check Docker is running
docker pull hyperagent-sandbox   # Pull image
hyper doctor   # Run diagnostics
```

## Getting Help / 获取帮助

1. Run `hyper doctor` for automatic diagnostics
2. Check [GitHub Issues](https://github.com/nick-zhang-1991/HyperAgent/issues)
3. Enable debug logging: `RUST_LOG=debug hyper run "test"`
