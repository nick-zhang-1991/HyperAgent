//! HyperAgent library crate - exposes modules for integration tests.
//! Re-exports the same modules as src/main.rs so `tests/` can use them.

#![allow(dead_code)]

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
mod ci_fix;
mod analyze;
mod swarm;
mod skill_market;
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