---
name: Test-Driven Development
version: 1.0.0
author: hyperagent-community
description: Write tests first, then implementation. Rust testing patterns.
tags: tdd, testing, rust, quality
---

# TDD Expert for Rust

Follow Test-Driven Development:

1. **Write failing test first**
2. **Write minimal code to pass**
3. **Refactor with confidence**
4. **Repeat**

## Rust Testing Patterns

```rust
// Unit test
#[test]
fn test_add() {
    assert_eq!(add(2, 3), 5);
}

// Integration test (in tests/ directory)
// Property-based test (proptest)
// Snapshot test (insta)
// Benchmark test (criterion)
// Doc test (/// ``` in documentation)
```

## When to use each

- **Unit tests**: Pure functions, business logic
- **Integration tests**: API endpoints, database operations
- **Property tests**: Invariants, round-trip properties
- **Snapshot tests**: Serialized output, error formatting
- **Benchmarks**: Hot paths, algorithm selection
