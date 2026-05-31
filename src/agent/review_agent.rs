use anyhow::Result;

use crate::diff::FileChange;
use crate::llm::{LlmProvider, Message};

/// ReviewAgent: Validates code changes with MERGED two-stage review
///
/// Optimization: spec compliance + code quality are done in a SINGLE LLM call
/// instead of two, cutting review latency by ~50%.
/// For trivial changes (<=2 files, <20 lines), quality review is skipped.
///
/// Changes must PASS BOTH stages to be approved.
pub struct ReviewAgent<'a> {
    provider: &'a LlmProvider,
}

impl<'a> ReviewAgent<'a> {
    pub fn new(provider: &'a LlmProvider) -> Self {
        Self { provider }
    }

    pub async fn review(
        &self,
        task: &str,
        changes: &[FileChange],
    ) -> Result<Vec<FileChange>> {
        if changes.is_empty() {
            return Ok(vec![]);
        }

        // Build review context
        let review_input = self.build_review_context(task, changes);

        // Determine if quality check is needed
        let trivial = changes.len() <= 2
            && changes.iter().all(|c| {
                let old_lines = c.old_content.as_deref().map(|s| s.lines().count()).unwrap_or(0);
                let new_lines = c.new_content.as_deref().map(|s| s.lines().count()).unwrap_or(0);
                let delta = old_lines.max(new_lines).saturating_sub(old_lines.min(new_lines));
                delta < 20
            });

        // MERGED REVIEW: one LLM call for both spec + quality
        println!("   🔍 Reviewing changes...");
        let merged_result = self.merged_review(&review_input, trivial).await?;

        // Parse results
        let mut final_changes: Vec<FileChange> = Vec::new();
        let mut rejected_reasons: Vec<String> = Vec::new();

        for (i, change) in changes.iter().enumerate() {
            let spec_pass = merged_result.spec_approved.contains(&i);
            let quality_pass = trivial || merged_result.quality_approved.contains(&i);

            if spec_pass && quality_pass {
                final_changes.push(change.clone());
            } else {
                let reason = if !spec_pass {
                    merged_result.spec_reasons.get(&i)
                        .cloned()
                        .unwrap_or_else(|| "Failed spec compliance".to_string())
                } else {
                    merged_result.quality_reasons.get(&i)
                        .cloned()
                        .unwrap_or_else(|| "Failed quality review".to_string())
                };
                rejected_reasons.push(format!(
                    "   ❌ Change {} ({}) REJECTED ({}): {}",
                    i,
                    change.file.display(),
                    if !spec_pass { "Spec" } else { "Quality" },
                    reason
                ));
            }
        }

        for r in &rejected_reasons {
            println!("{r}");
        }

        if final_changes.is_empty() {
            println!("   ⚠️  All changes rejected.");
        } else if !rejected_reasons.is_empty() {
            println!(
                "   Review: {} approved / {} total",
                final_changes.len(),
                changes.len(),
            );
        }

        Ok(final_changes)
    }

    /// Merged spec + quality review in one LLM call
    async fn merged_review(&self, review_input: &str, trivial: bool) -> Result<MergedReviewResult> {
        let quality_instruction = if trivial {
            "SKIP quality review — changes are trivial.".to_string()
        } else {
            r#"
=== STAGE 2: CODE QUALITY (Karpathy-informed) ===
For each change that passes spec, also check:
- SIMPLICITY: minimum code? no unnecessary abstractions?
- PRECISION: touch only what's needed? no formatting changes?
- SAFETY: any unwrap() panics? edge cases? security issues?
- CONSISTENCY: matching existing style? correct imports?"#.to_string()
        };

        let system_prompt = format!(
            r#"You are a code reviewer. Do a TWO-STAGE review in ONE response.

=== STAGE 1: SPEC COMPLIANCE ===
For each proposed change, does it directly solve the user's task?
REJECT if: incomplete, over-builds (YAGNI), wrong file, unfinished
APPROVED if: directly solves task, complete, nothing extra

{quality_instruction}

Output ONLY valid JSON:
{{
  "spec_approved": [0, 2],
  "spec_rejected": [1],
  "spec_reasons": {{"1": "Added caching layer not in spec"}},
  "quality_approved": [0, 2],
  "quality_rejected": [],
  "quality_reasons": {{}}
}}

If SPEC rejects a change, it's rejected regardless of quality.
Numbers are 0-based indices of the proposed changes."#
        );

        let response = self
            .provider
            .chat(vec![
                Message { 
                    role: "system".to_string(),
                    content: system_prompt,
                },
                Message { 
                    role: "user".to_string(),
                    content: review_input.to_string(),
                },
            ])
            .await?;

        Ok(self.parse_merged_response(&response))
    }

    fn build_review_context(&self, task: &str, changes: &[FileChange]) -> String {
        let mut ctx = format!("Task: {task}\n\nProposed changes:\n");

        for (i, change) in changes.iter().enumerate() {
            ctx.push_str(&format!(
                "\n--- Change {}: {} ({}) ---\n",
                i,
                change.file.display(),
                change.change_type
            ));

            if let Some(old) = &change.old_content {
                ctx.push_str(&format!("OLD:\n```\n{old}\n```\n"));
            }
            if let Some(new) = &change.new_content {
                ctx.push_str(&format!("NEW:\n```\n{new}\n```\n"));
            }
        }
        ctx
    }

    fn parse_merged_response(&self, response: &str) -> MergedReviewResult {
        let json_str = if let Some(start) = response.find('{') {
            if let Some(end) = response.rfind('}') {
                &response[start..=end]
            } else {
                eprintln!("   Warn: Review response missing closing brace - rejecting all");
                return MergedReviewResult::all_rejected("no closing brace");
            }
        } else {
            eprintln!("   Warn: Review response not JSON - rejecting all");
            return MergedReviewResult::all_rejected("non-JSON response");
        };

        match serde_json::from_str::<serde_json::Value>(json_str) {
            Ok(parsed) => {
                let parse_indices = |key: &str| -> Vec<usize> {
                    parsed[key].as_array()
                        .map(|arr| arr.iter()
                            .filter_map(|v| v.as_u64().map(|i| i as usize))
                            .collect())
                        .unwrap_or_default()
                };

                let parse_reasons = |key: &str| -> std::collections::HashMap<usize, String> {
                    parsed[key].as_object()
                        .map(|obj| obj.iter()
                            .filter_map(|(k, v)| {
                                k.parse::<usize>().ok()
                                    .map(|idx| (idx, v.as_str().unwrap_or("").to_string()))
                            })
                            .collect())
                        .unwrap_or_default()
                };

                MergedReviewResult {
                    spec_approved: parse_indices("spec_approved"),
                    spec_rejected: parse_indices("spec_rejected"),
                    spec_reasons: parse_reasons("spec_reasons"),
                    quality_approved: parse_indices("quality_approved"),
                    quality_rejected: parse_indices("quality_rejected"),
                    quality_reasons: parse_reasons("quality_reasons"),
                }
            }
            Err(e) => {
                eprintln!("   Warn: Review JSON parse error: {e} - rejecting all");
                MergedReviewResult::all_rejected(&format!("JSON error: {e}"))
            }
        }
    }
}

#[derive(Debug)]
struct MergedReviewResult {
    spec_approved: Vec<usize>,
    #[allow(dead_code)]
    spec_rejected: Vec<usize>,
    spec_reasons: std::collections::HashMap<usize, String>,
    quality_approved: Vec<usize>,
    #[allow(dead_code)]
    quality_rejected: Vec<usize>,
    quality_reasons: std::collections::HashMap<usize, String>,
}

impl MergedReviewResult {
    fn all_rejected(_reason: &str) -> Self {
        Self {
            spec_approved: Vec::new(),
            spec_rejected: Vec::new(),
            spec_reasons: std::collections::HashMap::new(),
            quality_approved: Vec::new(),
            quality_rejected: Vec::new(),
            quality_reasons: std::collections::HashMap::new(),
        }
    }
}
