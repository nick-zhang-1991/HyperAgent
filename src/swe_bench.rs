//! SWE-bench compatible evaluation — run HyperAgent against real GitHub issues
//!
//! A subset of SWE-bench Verified / Lite instances adapted for HyperAgent.
//! Each instance: a GitHub issue + the expected fix.
//!
//! Usage:
//!   cargo run -- eval --task swe-bench    # Run all SWE-bench instances
//!   cargo run -- eval --task swe-bench-lite  # Run a smaller subset

use crate::eval::{EvalCategory, EvalTask};

/// Return a set of SWE-bench-style evaluation tasks.
/// These are adapted from SWE-bench Verified / Lite instances.
pub fn swe_bench_tasks() -> Vec<EvalTask> {
    vec![
        EvalTask {
            name: "swe-bench-rust-async-fn".into(),
            category: EvalCategory::BugFix,
            prompt: "Fix the async function compilation error: the function `process_data` is declared `async` \
                     but it doesn't await anything. Make it a sync function instead.".into(),
            setup: Some(r#"
mkdir -p src
cat > Cargo.toml << 'EOF'
[package]
name = "swe-eval-async"
version = "0.1.0"
edition = "2021"
EOF
cat > src/lib.rs << 'EOF'
// BUG: async fn that doesn't actually await
pub async fn process_data(data: Vec<u8>) -> Vec<u8> {
    data.iter().map(|b| b + 1).collect()
}

pub fn greet(name: &str) -> String {
    format!("Hello, {}!", name)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_process() {
        let result = process_data(vec![1, 2, 3]);
        assert_eq!(result, vec![2, 3, 4]);
    }
}
EOF
"#.into()),
            expected_behavior: vec![
                "Code should compile with cargo check",
                "async should be removed from process_data",
                "All existing tests should pass",
            ],
            check_compile: true,
            check_tests: true,
            max_lines_threshold: 500,
            timeout_secs: 300,
        },

        EvalTask {
            name: "swe-bench-rust-clippy-fix".into(),
            category: EvalCategory::BugFix,
            prompt: "Fix all clippy warnings in the codebase. There are unused variables, \
                     unnecessary clones, and redundant pattern matching that need to be cleaned up.".into(),
            setup: Some(r#"
mkdir -p src
cat > Cargo.toml << 'EOF'
[package]
name = "swe-eval-clippy"
version = "0.1.0"
edition = "2021"
EOF
cat > src/lib.rs << 'EOF'
// BUG 1: Unused variable
pub fn calculate(a: i32, b: i32) -> i32 {
    let _unused = 42;
    a + b
}

// BUG 2: Unnecessary clone
pub fn process(items: &Vec<String>) -> Vec<String> {
    items.clone().iter().map(|s| s.clone()).collect()
}

// BUG 3: Redundant pattern
pub fn check_value(x: Option<i32>) -> i32 {
    match x {
        Some(v) => v,
        None => 0,
    }
}

// BUG 4: Single-character name
pub fn get_name() -> String {
    let a = "John";
    a.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_calc() { assert_eq!(calculate(2, 3), 5); }
    #[test]
    fn test_process() {
        let items = vec!["a".into(), "b".into()];
        assert_eq!(process(&items), vec!["a", "b"]);
    }
    #[test]
    fn test_check() { assert_eq!(check_value(Some(5)), 5); assert_eq!(check_value(None), 0); }
}
EOF
"#.into()),
            expected_behavior: vec![
                "cargo clippy should produce zero warnings",
                "All tests should pass",
                "No functionality should change",
            ],
            check_compile: true,
            check_tests: true,
            max_lines_threshold: 500,
            timeout_secs: 300,
        },

        EvalTask {
            name: "swe-bench-rust-error-handling".into(),
            category: EvalCategory::BugFix,
            prompt: "Fix the error handling: The read_file function should return proper Result types \
                     instead of panicking. Use anyhow or custom error types. Also fix the unwrap() calls.".into(),
            setup: Some(r#"
mkdir -p src
cat > Cargo.toml << 'EOF'
[package]
name = "swe-eval-errors"
version = "0.1.0"
edition = "2021"
[dependencies]
anyhow = "1.0"
EOF
cat > src/lib.rs << 'EOF'
use std::fs;
use std::path::Path;

// BUG: Panics on error instead of returning Result
pub fn read_file(path: &str) -> String {
    fs::read_to_string(path).unwrap()
}

// BUG: Unwrap that could panic
pub fn parse_number(s: &str) -> i32 {
    s.trim().parse::<i32>().unwrap()
}

// BUG: Panics if file doesn't exist
pub fn file_size(path: &str) -> u64 {
    fs::metadata(path).unwrap().len()
}

pub fn add(a: i32, b: i32) -> i32 { a + b }
EOF
"#.into()),
            expected_behavior: vec![
                "No unwrap() or expect() calls remaining",
                "All functions return proper Result types",
                "Code should compile",
            ],
            check_compile: true,
            check_tests: false,
            max_lines_threshold: 500,
            timeout_secs: 300,
        },

        EvalTask {
            name: "swe-bench-rust-buffer-overflow".into(),
            category: EvalCategory::BugFix,
            prompt: "Fix the buffer handling bug: the function should handle edge cases properly \
                     — empty input, very large input, and inputs where the buffer might overflow. \
                     Use safe Rust patterns (saturating arithmetic, checked indexing, etc.)".into(),
            setup: Some(r#"
mkdir -p src
cat > Cargo.toml << 'EOF'
[package]
name = "swe-eval-buffer"
version = "0.1.0"
edition = "2021"
EOF
cat > src/lib.rs << 'EOF'
// BUG: Panics on empty slice
pub fn first_element(data: &[i32]) -> i32 {
    data[0]
}

// BUG: No bounds checking on offset
pub fn get_slice(data: &[u8], offset: usize, count: usize) -> &[u8] {
    &data[offset..offset + count]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_first() { assert_eq!(first_element(&[1, 2, 3]), 1); }
    #[test]
    fn test_empty() {
        let empty: &[i32] = &[];
        // Should NOT panic
        let _ = first_element(empty);
    }
    #[test]
    fn test_slice() {
        let data = [1, 2, 3, 4, 5];
        assert_eq!(get_slice(&data, 0, 3), &[1, 2, 3]);
    }
}
EOF
"#.into()),
            expected_behavior: vec![
                "No indexing without bounds checking",
                "Edge cases (empty input) should be handled without panic",
                "All tests should pass",
            ],
            check_compile: true,
            check_tests: true,
            max_lines_threshold: 500,
            timeout_secs: 300,
        },

        // Python — broadest SWE-bench dataset coverage
        EvalTask {
            name: "swe-bench-python-import".into(),
            category: EvalCategory::BugFix,
            prompt: "Fix the Python import bug: the code has a circular import issue. \
                     The two modules import each other. Restructure them to eliminate \
                     the circular dependency.".into(),
            setup: Some(r#"
mkdir -p src
cat > src/main.py << 'EOF'
from src.module_a import func_a
def main():
    return func_a()
if __name__ == "__main__":
    print(main())
EOF
cat > src/module_a.py << 'EOF'
from src.module_b import func_b
def func_a():
    return f"a -> {func_b()}"
EOF
cat > src/module_b.py << 'EOF'
from src.module_a import func_a
def func_b():
    return "b"
EOF
"#.into()),
            expected_behavior: vec![
                "No circular imports",
                "python -c 'from src.main import main' should work",
            ],
            check_compile: false,
            check_tests: false,
            max_lines_threshold: 500,
            timeout_secs: 300,
        },

        EvalTask {
            name: "swe-bench-python-api-fix".into(),
            category: EvalCategory::BugFix,
            prompt: "Fix the Python async API: the fetch_data function has a bug — \
                     it doesn't properly handle HTTP errors. Add proper error handling \
                     with retry logic and timeout. Use asyncio.wait_for for timeout.".into(),
            setup: Some(r#"
mkdir -p src
cat > src/api.py << 'EOF'
import asyncio
async def fetch_data(url: str) -> dict:
    import aiohttp
    async with aiohttp.ClientSession() as session:
        async with session.get(url) as response:
            return await response.json()
EOF
"#.into()),
            expected_behavior: vec![
                "Add timeout using asyncio.wait_for",
                "Add proper error handling for HTTP errors",
                "Handle ConnectionError specifically",
            ],
            check_compile: false,
            check_tests: false,
            max_lines_threshold: 500,
            timeout_secs: 300,
        },

        EvalTask {
            name: "swe-bench-js-async".into(),
            category: EvalCategory::BugFix,
            prompt: "Fix the JavaScript async bug: The processItems function is NOT properly \
                     handling async operations — it returns before all items are processed. \
                     Use Promise.all to ensure all async operations complete.".into(),
            setup: Some(r#"
cat > src/index.js << 'EOF'
async function processItems(items) {
    items.forEach(async (item) => {
        await processItem(item);
    });
    console.log("All done!");
}
async function processItem(item) {
    return new Promise(resolve => {
        setTimeout(() => {
            console.log(`Processed ${item}`);
            resolve(item);
        }, 100);
    });
}
module.exports = { processItems, processItem };
EOF
"#.into()),
            expected_behavior: vec![
                "processItems should properly await all items",
                "Use Promise.all or for...of with await",
                "'All done!' should log AFTER all items are processed",
            ],
            check_compile: false,
            check_tests: false,
            max_lines_threshold: 500,
            timeout_secs: 300,
        },
    ]
}
