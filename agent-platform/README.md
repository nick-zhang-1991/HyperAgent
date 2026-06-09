# Agent Orchestration Platform

Multi-agent management platform powered by HyperAgent.

## Architecture

```
Desktop (Tauri v2) ───┐
Web (React+Vite)  ────┤
                       ├──→ Backend (Rust+axum) ← WebSocket real-time
CLI (hyperagent) ─────┘
```

## Quick Start

```bash
# 1. Start backend
cd agent-platform/backend
cargo run

# 2. Start frontend (web)
cd agent-platform/frontend
npm install && npm run dev

# 3. Open http://localhost:5173

# 4. Desktop (Mac/Windows)
cd agent-platform/frontend
npm run tauri dev
```

## Features

- **Organizations**: Create and manage multiple organizations
- **Agents**: Define agents with custom roles (natural language)
- **Tasks**: Assign tasks to agents using natural language
- **Real-time Dashboard**: WebSocket-powered live status updates
- **Desktop**: Native Mac/Windows app via Tauri v2
- **Web**: Browser-based UI via React+Vite
- **Powered by HyperAgent**: All agent capabilities from HyperAgent

## API Endpoints

| Method | Path | Description |
|--------|------|-------------|
| POST | /api/orgs | Create organization |
| GET | /api/orgs | List organizations |
| POST | /api/orgs/:id/agents | Create agent with role |
| GET | /api/orgs/:id/agents | List agents |
| POST | /api/orgs/:id/agents/:aid/tasks | Assign task |
| GET | /api/orgs/:id/agents/:aid/tasks | List tasks |
| GET | /api/ws | WebSocket real-time events |
