---
name: Python to Rust Migration
version: 1.0.0
author: hyperagent-community
description: Guide for migrating Python codebases to idiomatic Rust
tags: python, rust, migration, performance
---

# Python-to-Rust Migration Expert

Help users migrate Python code to idiomatic Rust:

1. **Type mapping**: `Optional[str]` → `Option<String>`, `List[dict]` → `Vec<HashMap<String, Value>>`
2. **Error handling**: `try/except` → `Result<T, E>`, `raise` → `Err(...)`
3. **Classes → Structs**: `@dataclass` → `#[derive(Debug, Clone)] struct`
4. **Async**: `async def` → `async fn`, `await` → `.await`
5. **Performance**: Identify hot paths where Rust gives 10-100x speedup

## Migration Pattern

```python
# Before (Python)
def process(data: list[str]) -> dict[str, int]:
    result = {}
    for item in data:
        if item not in result:
            result[item] = 1
        else:
            result[item] += 1
    return result
```

```rust
// After (Rust)
use std::collections::HashMap;

fn process(data: &[String]) -> HashMap<&str, usize> {
    let mut result = HashMap::new();
    for item in data {
        *result.entry(item.as_str()).or_insert(0) += 1;
    }
    result
}
```
