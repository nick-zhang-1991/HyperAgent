# HyperAgent Performance Baselines

## Overview

HyperAgent prioritizes low latency, minimal context, and high-throughput agentic
loops. This document captures the measured performance of internal hot paths
and the optimizations applied to them.

Benchmarks live as `#[test] #[ignore]` cases inside each module's `#[cfg(test)]`
block. They are **off by default** to keep `cargo test` fast; run with:

```bash
cargo test --offline --bin hyperagent -- --ignored --nocapture
```

All numbers below were captured on a single Apple Silicon dev machine in
**debug mode** (no optimizations, `--dev` profile, debuginfo=2). Expect
**5-10x** speedup in `--release` builds.

---

## Critical-path Baselines (debug build)

| Operation | Volume | Wall-clock | Per-op | Notes |
|---|---|---:|---:|---|
| `Message::text` construction | 1M | 4.1s | 4.1 µs | Includes heap alloc for `String` |
| `Message::text_content` extraction | 1M | 3.4s | 3.4 µs | 18MB sink total |
| `max_tokens_for` (adaptive) | 1M | 9.6s | 9.6 µs | Heuristic + scan over message tokens |
| `ChatRequest` JSON serialize | 100k | 21.8s | 218 µs | 44MB payload total |
| `LlmProvider::new` | 10k | 16.4s (before) → **317ms (after)** | 1.6 ms → **32 µs** | **52x faster** — shared client |
| `ProviderPool::new(100)` | 1k | 231s (before) → **2.59s (after)** | 231 ms → **2.6 ms** | **89x faster** — shared client |
| `record_failure` | 100k | 18ms | 184 ns | DashMap insert |
| `health_summary` (20 providers) | 10k | 524ms | 52 µs | Format + aggregation |
| `active_provider_name` | 1M | 973ms | 973 ns | String cloning |
| `ModeRegistry::default` | 10k | 381ms | 38 µs | HashMap insert × 5 modes |
| `build_prompt("code")` | 100k | 878ms | 9 µs | 75MB sink total |
| `ModeConfig` round-trip (serde) | 10k | 945ms | 94 µs | JSON serialize + deserialize |
| `ModeRegistry::get` | 1M | 2.5s | 2.5 µs | 605MB sink total |

---

## Optimization: Shared reqwest Client (`v0.1.x`)

### Problem

`LlmProvider::new` constructs a fresh `reqwest::Client` on every call. Each
client spins up:

- A TLS backend (ring or aws-lc-sys)
- A DNS resolver (trust-dns or system)
- A tokio runtime handle
- A connection pool (`HashMap<Host, ...>`)

In debug, this costs **~1.6 ms per provider**. With 100 providers in
`ProviderPool`, that's **~230 ms wasted on startup** doing redundant work.

### Solution

`src/llm/provider.rs` now uses a process-wide `OnceLock<Client>`:

```rust
fn shared_client() -> Client {
    static CLIENT: OnceLock<Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        Client::builder()
            .timeout(Duration::from_secs(300))
            .pool_max_idle_per_host(8)
            .build()
            .expect("infallible")
    }).clone()
}
```

All `LlmProvider::new` and `LlmProvider::from_env_or` callsites now invoke
`shared_client()` instead of constructing a fresh client.

### Why this is safe

- `reqwest::Client` is internally `Arc`-backed. `.clone()` is a refcount
  bump, not a deep copy.
- All `LlmProvider`s in a process share identical HTTP configuration
  (timeout=300s, pool_max_idle_per_host=8). The shared client matches both.
- Base URL and API key are request-level (passed per `.post(url)` call), not
  client-level. A single client can route to any host.
- `OnceLock::get_or_init` guarantees exactly-once initialization across
  threads, even under concurrent first-access races.
- No external dep added — `std::sync::OnceLock` is stable since Rust 1.70;
  project MSRV is 1.78.

### Expected impact

- `LlmProvider::new`: **1.6 ms → ~5 µs** (~300x speedup)
- `ProviderPool::new(100)`: **231 ms → ~10 ms** (~23x speedup, dominated by
  100 struct allocations rather than 100 reqwest constructions)
- Binary startup: every command that touches an LLM provider
  (`hyper run`, `hyper review`, `hyper init` when LLM is configured) now
  saves 1-230 ms depending on pool size.

### Verification

Run the optimized benches:

```bash
cargo test --offline --bin hyperagent -- --ignored \
    llm::provider::tests::bench_provider_construction_throughput \
    llm::pool::tests::bench_pool_creation_with_many_providers \
    --nocapture
```

---

## Optimization: Mock Server Race Fix

`MockLlmServer::start()` in `src/llm/mock_server.rs` spawns a thread that
binds a TCP listener and serves `/v1/chat/completions` requests. Tests that
use it must wait for the listener to become ready.

**Pre-fix**: 50ms `thread::sleep` after `MockLlmServer::start()`. Failed
~20% of the time on slow CI / under parallel-test load.

**Post-fix**: 200ms `thread::sleep` in 3 test callsites
(`test_mock_server_responds`, `test_llm_provider_with_mock`,
`test_mock_stream_connection_works`).

Trade-off: 150ms slower per mock test × 3 tests = +450ms CI time. Worth it
for eliminating flakiness.

---

## Future Optimizations (not yet implemented)

1. **Connection warming**: pre-establish TLS connections to primary LLM
   providers on first `LlmProvider::new` to remove the first-request
   cold-start tax (~50-200ms for new TLS handshakes).

2. **JSON buffer pool**: `ChatRequest` and `Message` serializations
   allocate fresh `String` buffers 100k+ times. A `Vec<u8>` pool keyed on
   capacity would cut allocator pressure. Estimated 20% wall-clock savings
   on `ChatRequest` serialize.

3. **Lazy mode prompt loading**: `ModeRegistry::default()` inserts 5
   full system prompts at construction. Lazy-loading on first
   `build_prompt(mode)` call would defer ~190 µs of allocations off the
   startup critical path.

4. **Provider pool lazy init**: `ProviderPool::new(n)` eagerly constructs
   all `n` providers. Lazy-constructing on first use would make
   `ProviderPool::new(100)` effectively O(1) for startup.

---

## Adding New Benchmarks

Pattern: a single `#[test] #[ignore]` function with `std::time::Instant`:

```rust
#[test]
#[ignore] // excluded from default `cargo test`
fn bench_my_hot_path_throughput() {
    use std::time::Instant;
    let n = 1_000_000;
    let start = Instant::now();
    for _ in 0..n {
        let _ = do_work();
    }
    let elapsed = start.elapsed();
    println!("my_op x{n}: {:.2?} ({:.0} ns/op)",
        elapsed, elapsed.as_nanos() as f64 / n as f64);
    // No hard assert — debug timings vary 30%+ under CI load.
    // Numbers are advisory; humans review the trend.
}
```

Add a row to the **Critical-path Baselines** table above when you add
a new bench. Keep the "Bottleneck" marker for any operation > 1ms/op.
