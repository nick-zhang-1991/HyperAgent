//! HyperAgent - Ultra-Fast CLI Coding Agent
//!
//! Key differentiators:
//! 1. **TurboIndex** - Tree-sitter based global code index with PageRank relevance
//! 2. **Multi-Agent Pipeline** - Concurrent specialized agents working in parallel
//! 3. **Streaming-First** - All LLM calls stream, no waiting for full responses
//! 4. **Minimal Context** - Only sends the most relevant code to the LLM
//! 5. **Incremental Updates** - Watches filesystem, re-indexes only changed files
//!
mod agent;
mod agent_graph;
mod auth;
mod budget_tracker;
mod checkpoint;
mod cli;
mod context_dashboard;
mod dep_graph;
mod diff;
mod diff_view;
mod eval;
mod git;
mod health;
mod hooks;
mod index;
mod kanban;
mod knowledge;
mod llm;
mod mcp;
mod mcp_server;
mod memory;
mod modes;
mod multi_repo;
mod notify;
mod organization;
mod plugins;
mod refactor;
mod repl;
mod router;
mod rules;
mod scaffold;
mod scheduler;
mod security;
mod session;
mod spinner;
mod swe_bench;
mod telemetry;
mod test_gen;
mod test_runner;
#[cfg(feature = "tui")]
mod tui;
mod web_search;
mod web_ui;
use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("hyperagent=warn")),
        )
        .init();

    let cli = cli::Cli::parse();
    cli.run().await
}
