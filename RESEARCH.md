# HyperAgent Research - CLI Agent Comparison

## opencode (v1.14.41, Node.js launcher + native binary)
- Multi-agent: primary + subagent modes
- Granular permissions: edit/bash/task/webfetch per agent
- Session: continue, fork, share
- MCP server integration
- Provider abstraction with per-agent model override
- Smart agent naming: build, plan, explore, general

## goose (Go, 247MB binary)
- Full TUI experience  
- MCP integration
- Extensive tool ecosystem

## deepseek-tui (Rust, 38MB binary)
- git diff code review
- GitHub PR integration
- Session resume/fork
- Sandbox execution
- Patch file apply
- AGENTS.md for project context
- Doctor/diagnostics
- MCP server management
- Offline evaluation harness

## hermes (Rust/Python)
- Smart model routing (simple/complex patterns)
- Multi-provider with fallback
- Skills system (procedural memory)
- Cron jobs
- TUI mode
- MCP support

## Features to integrate into HyperAgent:
1. Agent permission system (opencode)
2. Session save/resume/fork (opencode + deepseek-tui)
3. Git diff code review (deepseek-tui)
4. PR integration (deepseek-tui)
5. AGENTS.md for project context (deepseek-tui)
6. Smart model routing (hermes)
7. Multi-provider with fallback (hermes)
8. MCP integration (all)
9. Sandbox execution (deepseek-tui)
10. Doctor/diagnostics (deepseek-tui)
11. Patch file apply (deepseek-tui)
12. JSON session export/import (opencode)
