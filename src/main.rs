//! HyperAgent - Ultra-Fast CLI Coding Agent
#![allow(dead_code)]
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
mod analytics;
mod bench;
mod billing;
mod cli;
mod computer_use_cross;
mod config;
mod dep_graph;
mod diff;
mod diff_view;
mod error_recovery;
mod eval;
mod git;
mod hooks;
mod i18n;
mod index;
mod kanban;
mod keychain;
mod knowledge;
mod llm;
mod llm_cache;
mod mcp;
mod memory;
mod metrics;
mod modes;
mod onboarding;
mod plugin;
mod refactor;
mod repl;
mod retrieval;
mod router;
mod serve;
mod sandbox;
mod scaffold;
mod scaffold_templates;
mod security;
mod session;
mod sync;
mod spinner;
mod team;
mod saas;
mod test_gen;
mod test_runner;
#[cfg(feature = "tui")]
mod tui;
mod updater;
mod web_search;
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

    // Initialize i18n (locale detection from LANG env or HYPER_LANG override)
    crate::i18n::init(crate::i18n::Locale::detect());

    // Initialize metrics
    crate::metrics::Metrics::init();

    let cli = cli::Cli::parse();
    cli.run().await
}
