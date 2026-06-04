# HyperAgent for VS Code

AI coding agent with parallel multi-agent pipeline — right in VS Code.

## Features

- 🤖 **Chat with HyperAgent** — full agent capabilities in VS Code
- 🔍 **Explain Code** — select code, get instant analysis
- 🛠️ **Fix Errors** — auto-fix build and lint errors
- 🚀 **Run Prompts** — execute any HyperAgent prompt
- 💬 **Side Panel** — persistent chat interface
- 🔌 **MCP Bridge** — connects to HyperAgent's MCP server via stdio

## Installation

### From VS Code Marketplace

1. Open VS Code
2. Go to Extensions (Ctrl+Shift+X / Cmd+Shift+X)
3. Search for "HyperAgent"
4. Click Install

### Manual Installation (.vsix)

1. Download `hyperagent-vscode.vsix` from the [latest release](https://github.com/nick-zhang-1991/HyperAgent/releases)
2. In VS Code, go to Extensions → ... (three dots) → Install from VSIX...
3. Select the downloaded file

### Cline Users

If you use [Cline](https://cline.bot), you can connect HyperAgent as an MCP server:

1. Open Cline settings → MCP Servers
2. Import [cline-mcp.json](./cline-mcp.json)
3. HyperAgent's tools will appear in Cline

## Requirements

- [HyperAgent](https://github.com/nick-zhang-1991/HyperAgent) installed and available in PATH
- VS Code 1.85.0 or later

## Commands

| Command | Description |
|---------|-------------|
| `HyperAgent: Open Chat` | Open the chat panel (Cmd+Shift+H) |
| `HyperAgent: Run Prompt` | Run a prompt in the current workspace |
| `HyperAgent: Explain Code` | Explain the selected code |
| `HyperAgent: Fix Errors` | Fix build/lint errors in the current file |

## Extension Settings

This extension connects to HyperAgent via the MCP stdio protocol.
HyperAgent must be installed and accessible via the `hyper` command.

## Release Notes

### 0.1.0
Initial release with chat panel, explain code, fix errors, and MCP bridge.
