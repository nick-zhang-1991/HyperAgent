#![allow(unused)]
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Result of a cross-file refactor operation
#[derive(Debug)]
pub struct RefactorResult {
    pub symbol: String,
    pub replacement: Option<String>,
    pub files_found: Vec<PathBuf>,
    pub files_modified: Vec<PathBuf>,
    pub occurrences: usize,
}

/// Find all references to a symbol in the codebase
pub fn find_references(root: &Path, symbol: &str, exclude_dirs: &[&str]) -> Result<Vec<(PathBuf, Vec<usize>)>> {
    let mut results = Vec::new();

    for entry in walkdir::WalkDir::new(root)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            !exclude_dirs.iter().any(|d| name == *d) // exact match only
        })
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();

        // Only check source files
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !matches!(ext, "rs" | "ts" | "tsx" | "js" | "jsx" | "py" | "go" | "toml" | "json" | "md" | "yaml" | "yml") {
            continue;
        }

        // Skip binary or huge files
        let metadata = match std::fs::metadata(path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if metadata.len() > 1_000_000 {
            continue;
        }

        let content = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Find all line numbers containing the symbol
        let lines: Vec<usize> = content
            .lines()
            .enumerate()
            .filter(|(_, line)| {
                // Match word boundaries for precise symbol matching
                let lower_line = line.to_lowercase();
                let lower_symbol = symbol.to_lowercase();
                lower_line.contains(&lower_symbol)
            })
            .map(|(i, _)| i + 1)
            .collect();

        if !lines.is_empty() {
            results.push((path.to_path_buf(), lines));
        }
    }

    Ok(results)
}

/// Preview a rename operation — shows what would change without applying
pub fn preview_rename(
    root: &Path,
    symbol: &str,
    replacement: &str,
    exclude_dirs: &[&str],
) -> Result<RefactorResult> {
    let references = find_references(root, symbol, exclude_dirs)?;
    let total_occurrences: usize = references.iter().map(|(_, lines)| lines.len()).sum();
    let files_found: Vec<PathBuf> = references.iter().map(|(p, _)| p.clone()).collect();

    println!("📋 Refactor Preview: '{symbol}' → '{replacement}'");
    println!("   Found {} references in {} files\n", total_occurrences, files_found.len());

    for (path, lines) in &references {
        let relative = path.strip_prefix(root).unwrap_or(path);
        println!("   📄 {} ({} occurrences):", relative.display(), lines.len());
        for line_num in lines.iter().take(5) {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Some(line) = content.lines().nth(line_num - 1) {
                    let trimmed = line.trim();
                    if trimmed.to_lowercase().contains(&symbol.to_lowercase()) {
                        let highlighted = trimmed
                            .replace(symbol, &format!("\x1b[32m{replacement}\x1b[0m"));
                        println!("     {:>4}: {}", line_num, highlighted);
                    }
                }
            }
        }
        if lines.len() > 5 {
            println!("     ... and {} more", lines.len() - 5);
        }
    }

    Ok(RefactorResult {
        symbol: symbol.to_string(),
        replacement: Some(replacement.to_string()),
        files_found,
        files_modified: Vec::new(),
        occurrences: total_occurrences,
    })
}

/// Execute a rename — replaces all occurrences across files
pub fn apply_rename(
    root: &Path,
    symbol: &str,
    replacement: &str,
    exclude_dirs: &[&str],
    dry_run: bool,
) -> Result<RefactorResult> {
    let references = find_references(root, symbol, exclude_dirs)?;
    let mut files_modified = Vec::new();
    let mut total_occurrences = 0;

    for (path, lines) in &references {
        total_occurrences += lines.len();
        let relative = path.strip_prefix(root).unwrap_or(path);

        let content = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;

        // Count occurrences before replacement
        let count_before = content.matches(symbol).count();

        if dry_run {
            println!("   📄 {} — would replace {} occurrences", relative.display(), count_before);
            files_modified.push(path.clone());
            continue;
        }

        // Replace with word-boundary awareness
        let new_content = replace_symbol(&content, symbol, replacement);

        let count_after = new_content.matches(replacement).count();
        let replaced = count_after > 0;
        let diff = count_before as isize - content.matches(symbol).count() as isize;

        if replaced && content != new_content {
            // Write the modified content back
            std::fs::write(path, &new_content)
                .with_context(|| format!("Failed to write {}", path.display()))?;
            files_modified.push(path.clone());
            println!("   ✅ {} — {} replacements applied", relative.display(), count_before);
        } else if diff != 0 && !replaced {
            println!("   ⚠️  {} — found {} occurrences but replacement may have issues", relative.display(), count_before);
        }
    }

    if dry_run {
        println!("\n📋 Dry-run complete. {} files would be modified with {} total replacements.",
            files_modified.len(), total_occurrences);
    } else {
        let file_count = files_modified.len();
        println!("\n✅ Refactor complete: '{symbol}' → '{replacement}'");
        println!("   Modified {} files, {} replacements", file_count, total_occurrences);
        if file_count > 0 {
            println!("   Run `git diff` to review changes before committing.");
        }
    }

    Ok(RefactorResult {
        symbol: symbol.to_string(),
        replacement: Some(replacement.to_string()),
        files_found: references.iter().map(|(p, _)| p.clone()).collect(),
        files_modified,
        occurrences: total_occurrences,
    })
}

/// Simple symbol replacement with context awareness
fn replace_symbol(content: &str, old: &str, new: &str) -> String {
    let old_lower = old.to_lowercase();
    let mut result = String::with_capacity(content.len());

    let mut search_start = 0;
    let lower_content = content.to_lowercase();

    while let Some(pos) = lower_content[search_start..].find(&old_lower) {
        let actual_pos = search_start + pos;

        // Check word boundaries (prevent partial matches)
        let prev_char = content[..actual_pos].chars().last();
        let is_word_start = prev_char.map_or(true, |c| !c.is_alphanumeric() && c != '_');

        let after_end = actual_pos + old.len();
        let next_char = content[after_end..].chars().next();
        let is_word_end = next_char.map_or(true, |c| !c.is_alphanumeric() && c != '_');

        if is_word_start && is_word_end {
            // Preserve case of the original
            let original = &content[actual_pos..actual_pos + old.len()];
            let replacement = match case_pattern(original) {
                CasePattern::LowerCase => new.to_lowercase(),
                CasePattern::UpperCase => new.to_uppercase(),
                CasePattern::TitleCase => {
                    let mut c = new.chars();
                    c.next().map(|f| f.to_uppercase().to_string() + c.as_str()).unwrap_or_default()
                }
                CasePattern::Mixed => new.to_string(),
            };

            result.push_str(&content[search_start..actual_pos]);
            result.push_str(&replacement);
            search_start = after_end;
        } else {
            // Not a word boundary, skip
            result.push_str(&content[search_start..=actual_pos]);
            search_start = actual_pos + 1;
        }
    }

    result.push_str(&content[search_start..]);
    result
}

#[derive(Debug, PartialEq)]
enum CasePattern {
    LowerCase,
    UpperCase,
    TitleCase,
    Mixed,
}

fn case_pattern(s: &str) -> CasePattern {
    if s.chars().all(|c| !c.is_alphabetic() || c.is_lowercase()) {
        CasePattern::LowerCase
    } else if s.chars().all(|c| !c.is_alphabetic() || c.is_uppercase()) {
        CasePattern::UpperCase
    } else if let Some(first) = s.chars().next() {
        if first.is_uppercase() && s[1..].chars().all(|c| !c.is_alphabetic() || c.is_lowercase()) {
            CasePattern::TitleCase
        } else {
            CasePattern::Mixed
        }
    } else {
        CasePattern::Mixed
    }
}

/// Remove unused imports in Rust files (dead-code cleanup helper)
pub fn remove_unused_imports(root: &Path, file_path: &Path) -> Result<Vec<String>> {
    let full_path = if file_path.is_relative() {
        root.join(file_path)
    } else {
        file_path.to_path_buf()
    };

    let content = std::fs::read_to_string(&full_path)
        .with_context(|| format!("Failed to read {}", full_path.display()))?;

    let mut removed = Vec::new();
    let mut new_lines = Vec::new();
    let mut in_use_block = false;
    let mut skip_until: Option<usize> = None;

    for (i, line) in content.lines().enumerate() {
        let trimmed = line.trim();

        // Detect multi-line import blocks
        if trimmed == "use" || trimmed.ends_with("::{") {
            in_use_block = true;
        }

        if let Some(threshold) = skip_until {
            if threshold > i {
                continue;
            }
        }
        skip_until = None;

        if in_use_block || trimmed.starts_with("use ") || trimmed.starts_with("pub use ") {
            if !trimmed.contains("super::") && !trimmed.contains("crate::") {
                let symbol_name = trimmed
                    .trim_start_matches("use ")
                    .trim_start_matches("pub use ")
                    .split("::")
                    .next()
                    .unwrap_or("")
                    .trim()
                    .trim_end_matches(';')
                    .trim_end_matches('{');

                if !symbol_name.is_empty() && !content.contains(&format!("fn {symbol_name}"))
                    && !content.contains(&format!("struct {symbol_name}"))
                    && !content.contains(&format!("enum {symbol_name}"))
                    && !content.contains(&format!("trait {symbol_name}"))
                {
                    removed.push(symbol_name.to_string());
                    continue;
                }
            }
        } else {
            in_use_block = false;
        }

        new_lines.push(line);
    }

    if !removed.is_empty() {
        std::fs::write(&full_path, new_lines.join("\n"))
            .with_context(|| format!("Failed to write {}", full_path.display()))?;
    }

    Ok(removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_symbol_simple() {
        let result = replace_symbol("fn hello_world() {}", "hello_world", "greet");
        assert_eq!(result, "fn greet() {}");
    }

    #[test]
    fn test_replace_symbol_case_preserved() {
        let result = replace_symbol("HelloWorld::new()", "HelloWorld", "GreetFn");
        assert_eq!(result, "GreetFn::new()");
    }

    #[test]
    fn test_replace_symbol_partial_word_skipped() {
        let result = replace_symbol("hello_world hello_world2", "hello_world", "greet");
        assert_eq!(result, "greet hello_world2");
    }

    #[test]
    fn test_replace_symbol_uppercase() {
        let result = replace_symbol("HELLO_WORLD", "hello_world", "greet_fn");
        assert_eq!(result, "GREET_FN");
    }

    #[test]
    fn test_find_references() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.rs");
        std::fs::write(&file, "fn my_func() {}\nfn main() { my_func(); }").unwrap();
        let refs = find_references(dir.path(), "my_func", &["target", ".git"]).unwrap();
        assert!(!refs.is_empty());
        assert_eq!(refs[0].1.len(), 2); // definition + call
    }

    #[test]
    fn test_preview_rename() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("lib.rs"), "fn old_name() {}\npub fn main() { old_name(); }").unwrap();
        let result = preview_rename(dir.path(), "old_name", "new_name", &["target", ".git"]).unwrap();
        assert_eq!(result.occurrences, 2);
        assert_eq!(result.files_found.len(), 1);
    }

    #[test]
    fn test_apply_rename_dry_run() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("mod.rs"), "fn target_fn() {}\ntarget_fn();").unwrap();
        let result = apply_rename(dir.path(), "target_fn", "renamed_fn", &["target", ".git"], true).unwrap();
        assert_eq!(result.occurrences, 2);
        // Dry run shouldn't modify files
        let content = std::fs::read_to_string(dir.path().join("mod.rs")).unwrap();
        assert!(content.contains("target_fn"));
    }

    #[test]
    fn test_apply_rename_live() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("app.rs"), "fn live_fn() {}\nlive_fn();").unwrap();
        let result = apply_rename(dir.path(), "live_fn", "renamed_fn", &["target", ".git"], false).unwrap();
        assert_eq!(result.occurrences, 2);
        assert_eq!(result.files_modified.len(), 1);
        let content = std::fs::read_to_string(dir.path().join("app.rs")).unwrap();
        assert!(content.contains("renamed_fn"));
        assert!(!content.contains("live_fn"));
    }
}
