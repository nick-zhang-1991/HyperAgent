# HyperAgent

## Project Structure
- `/src/cli.rs` - CLI entry with subcommands (run, review, init, stats, session, doctor, config, agents)
- `/src/agent/` - Multi-agent pipeline (orchestrator + plan/code/review/apply agents)
- `/src/index/` - Global code index with tree-sitter parsing, SQLite cache, PageRank
- `/src/llm/` - LLM provider abstraction (OpenAI-compatible API)
- `/src/session.rs` - Session management (save/resume/fork)
- `/src/diff/` - Diff parsing and file change application
- `/src/git/` - Git operations

## Code Style
- Rust 2021 edition
- async/await with tokio runtime
- anyhow for error handling
- serde for serialization
- tracing for logging

## Build
cargo build --release  # Release build
cargo check            # Fast validation
cargo test             # Run tests

## Usage
hyper run "prompt"     # Run coding task
hyper init             # Build index
hyper review           # Code review
hyper doctor           # Diagnostics
