//! LLM provider abstraction layer.
//!
//! HyperAgent speaks to multiple LLM backends through a single
//! [`LlmProvider`] interface, which is an OpenAI-compatible chat
//! completions client. A [`ProviderPool`] aggregates several providers
//! with health tracking and automatic failover.
//!
//! # Architecture
//!
//! - [`provider`]: low-level chat client, request/response types, retry
//!   loop, and adaptive `max_tokens` calculation. All providers share a
//!   process-wide `reqwest::Client` (see
//!   [`provider::shared_client`]) for amortized connection-pool cost.
//! - [`pool`]: multi-provider registry with health scoring, circuit
//!   breaker, and round-robin failover across healthy providers.
//! - [`streaming`]: SSE/streaming response parser.
//! - [`mock_server`]: lightweight HTTP server that returns canned
//!   completions, used by integration tests.
//!
//! # Performance
//!
//! Hot-path baselines and optimizations are documented in
//! `docs/PERFORMANCE.md`. Key wins:
//!
//! - `LlmProvider::new` reuses a process-wide `reqwest::Client` — see
//!   [`provider::shared_client`].
//! - `ProviderPool::new(n)` is O(n) struct allocations, not O(n) TLS
//!   backends.

pub mod provider;
pub mod streaming;
pub mod pool;
#[cfg(test)]
pub mod mock_server;

pub use provider::LlmProvider;
pub use provider::{ContentPart, Message};
pub use pool::ProviderPool;
