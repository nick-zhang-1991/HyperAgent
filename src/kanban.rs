//! Kanban Board — multi-agent parallel task execution
//!
//! Inspired by cline's Kanban system.
//!
//! Design:
//! - **Task Board** — cards with status (todo/in_progress/done/blocked)
//! - **Dependency Chaining** — card B depends on card A finishing first
//! - **Parallel Execution** — cards without unmet deps run simultaneously
//! - **Worktree Isolation** — each card gets its own git worktree
//! - **Auto-Commit** — completed cards auto-commit their changes
//! - **Agent Assignment** — each card is executed by an agent (mode-specific)

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;
use uuid::Uuid;

/// Status of a kanban card
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CardStatus {
    Todo,
    InProgress,
    Done,
    Blocked,
    Failed,
}

/// Priority level
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Critical,
    High,
    Medium,
    Low,
}

/// A single card/task on the kanban board
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Card {
    pub id: String,
    pub title: String,
    pub description: String,
    pub status: CardStatus,
    pub priority: Priority,
    pub agent_mode: String,
    pub dependencies: Vec<String>,          // card IDs this depends on
    pub dependents: Vec<String>,            // card IDs that depend on this
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub assigned_agent_id: Option<String>,
    pub worktree: Option<PathBuf>,
    pub result: Option<CardResult>,
    pub tags: Vec<String>,
    pub estimated_cost: Option<f64>,        // estimated token cost
}

/// Result of a completed card
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CardResult {
    pub summary: String,
    pub files_changed: Vec<String>,
    pub tokens_used: u64,
    pub exit_code: i32,
}

/// The kanban board
pub struct KanbanBoard {
    /// All cards, keyed by ID
    cards: Arc<Mutex<HashMap<String, Card>>>,
    /// Order of card IDs (for display)
    card_order: Arc<Mutex<Vec<String>>>,
    /// Project root
    project_root: PathBuf,
    /// Max parallel agents
    pub max_concurrency: usize,
}

impl KanbanBoard {
    pub fn new(project_root: &Path, max_concurrency: usize) -> Self {
        Self {
            cards: Arc::new(Mutex::new(HashMap::new())),
            card_order: Arc::new(Mutex::new(Vec::new())),
            project_root: project_root.to_path_buf(),
            max_concurrency,
        }
    }

    /// Add a card to the board
    pub async fn add_card(&self, title: &str, description: &str, priority: Priority, agent_mode: &str, dependencies: Vec<String>, tags: Vec<String>) -> String {
        let id = Uuid::new_v4().to_string();
        let card = Card {
            id: id.clone(),
            title: title.to_string(),
            description: description.to_string(),
            status: CardStatus::Todo,
            priority,
            agent_mode: agent_mode.to_string(),
            dependencies,
            dependents: Vec::new(),
            created_at: Utc::now(),
            started_at: None,
            completed_at: None,
            assigned_agent_id: None,
            worktree: None,
            result: None,
            tags,
            estimated_cost: None,
        };

        let mut cards = self.cards.lock().await;
        let mut order = self.card_order.lock().await;

        // Register this card as dependent on its dependencies
        for dep_id in &card.dependencies {
            if let Some(dep_card) = cards.get_mut(dep_id) {
                dep_card.dependents.push(card.id.clone());
            }
        }

        cards.insert(id.clone(), card);
        order.push(id.clone());
        id
    }

    /// Get cards ready to execute (no unmet dependencies)
    pub async fn get_ready_cards(&self) -> Vec<Card> {
        let cards = self.cards.lock().await;
        let in_progress_count = cards.values().filter(|c| c.status == CardStatus::InProgress).count();
        let available_slots = self.max_concurrency.saturating_sub(in_progress_count);

        if available_slots == 0 {
            return Vec::new();
        }

        let mut ready: Vec<Card> = cards.values()
            .filter(|c| c.status == CardStatus::Todo)
            .filter(|c| {
                c.dependencies.iter().all(|dep_id| {
                    cards.get(dep_id).is_some_and(|dep| dep.status == CardStatus::Done)
                })
            })
            .cloned()
            .collect();

        // Sort by priority
        ready.sort_by(|a, b| {
            let prio_val = |p: &Priority| -> u8 {
                match p {
                    Priority::Critical => 0,
                    Priority::High => 1,
                    Priority::Medium => 2,
                    Priority::Low => 3,
                }
            };
            prio_val(&a.priority).cmp(&prio_val(&b.priority))
        });

        ready.truncate(available_slots);
        ready
    }

    /// Mark a card as in-progress
    pub async fn start_card(&self, id: &str, agent_id: &str, worktree: PathBuf) -> anyhow::Result<()> {
        let mut cards = self.cards.lock().await;
        if let Some(card) = cards.get_mut(id) {
            card.status = CardStatus::InProgress;
            card.started_at = Some(Utc::now());
            card.assigned_agent_id = Some(agent_id.to_string());
            card.worktree = Some(worktree);
            Ok(())
        } else {
            anyhow::bail!("Card '{id}' not found");
        }
    }

    /// Mark a card as done
    pub async fn complete_card(&self, id: &str, result: CardResult) -> anyhow::Result<()> {
        let mut cards = self.cards.lock().await;
        if let Some(card) = cards.get_mut(id) {
            card.status = CardStatus::Done;
            card.completed_at = Some(Utc::now());
            card.result = Some(result);
            Ok(())
        } else {
            anyhow::bail!("Card '{id}' not found");
        }
    }

    /// Mark a card as blocked
    pub async fn block_card(&self, id: &str) -> anyhow::Result<()> {
        let mut cards = self.cards.lock().await;
        if let Some(card) = cards.get_mut(id) {
            card.status = CardStatus::Blocked;
            Ok(())
        } else {
            anyhow::bail!("Card '{id}' not found");
        }
    }

    /// Mark a card as failed
    pub async fn fail_card(&self, id: &str, reason: &str) -> anyhow::Result<()> {
        let mut cards = self.cards.lock().await;
        if let Some(card) = cards.get_mut(id) {
            card.status = CardStatus::Failed;
            card.result = Some(CardResult {
                summary: format!("Failed: {reason}"),
                files_changed: Vec::new(),
                tokens_used: 0,
                exit_code: 1,
            });
            Ok(())
        } else {
            anyhow::bail!("Card '{id}' not found");
        }
    }

    /// Get the current execution graph as a DOT string (for visualization)
    pub async fn to_dot(&self) -> String {
        let cards = self.cards.lock().await;
        let mut dot = String::from("digraph G {\n  rankdir=LR;\n  node [shape=box, style=rounded];\n");

        for card in cards.values() {
            let color = match card.status {
                CardStatus::Todo => "lightgray",
                CardStatus::InProgress => "lightblue",
                CardStatus::Done => "lightgreen",
                CardStatus::Blocked => "orange",
                CardStatus::Failed => "salmon",
            };
            let label = card.title.replace('"', "\\\"");
            dot.push_str(&format!("  \"{}\" [fillcolor={color}, style=filled, label=\"{label}\"];\n", card.id));
        }

        for card in cards.values() {
            for dep_id in &card.dependencies {
                dot.push_str(&format!("  \"{dep_id}\" -> \"{}\";\n", card.id));
            }
        }

        dot.push_str("}\n");
        dot
    }

    /// Get a clone of the shared card map for cross-thread use
    pub fn board_clone(&self) -> Arc<Mutex<HashMap<String, Card>>> {
        self.cards.clone()
    }

    /// Get overall status summary
    pub async fn summary(&self) -> KanbanSummary {
        let cards = self.cards.lock().await;
        let mut summary = KanbanSummary::default();

        for card in cards.values() {
            summary.total += 1;
            match card.status {
                CardStatus::Todo => summary.todo += 1,
                CardStatus::InProgress => summary.in_progress += 1,
                CardStatus::Done => summary.done += 1,
                CardStatus::Blocked => summary.blocked += 1,
                CardStatus::Failed => summary.failed += 1,
            }
            summary.total_tokens += card.result.as_ref().map(|r| r.tokens_used).unwrap_or(0);
        }
        summary
    }

    /// List all cards
    pub async fn list_cards(&self) -> Vec<Card> {
        let cards = self.cards.lock().await;
        let order = self.card_order.lock().await;
        order.iter()
            .filter_map(|id| cards.get(id).cloned())
            .collect()
    }

    /// Get a single card
    pub async fn get_card(&self, id: &str) -> Option<Card> {
        let cards = self.cards.lock().await;
        cards.get(id).cloned()
    }

    /// Clear the board
    pub async fn clear(&self) {
        let mut cards = self.cards.lock().await;
        let mut order = self.card_order.lock().await;
        cards.clear();
        order.clear();
    }
}

/// Summary statistics for the kanban board
#[derive(Debug, Default, Serialize, Deserialize)]
pub struct KanbanSummary {
    pub total: usize,
    pub todo: usize,
    pub in_progress: usize,
    pub done: usize,
    pub blocked: usize,
    pub failed: usize,
    pub total_tokens: u64,
}

// ═══════════════════════════════════════════════
// CLI helper for kanban rendering
// ═══════════════════════════════════════════════

impl KanbanBoard {
    /// Render a text-based board to the terminal
    pub async fn render(&self) -> String {
        let cards = self.cards.lock().await;
        let order = self.card_order.lock().await;

        let mut output = String::new();

        let columns = vec![
            ("📋 TODO", CardStatus::Todo),
            ("🔧 In Progress", CardStatus::InProgress),
            ("✅ Done", CardStatus::Done),
            ("🚫 Blocked", CardStatus::Blocked),
            ("❌ Failed", CardStatus::Failed),
        ];

        for (col_name, col_status) in columns {
            let col_cards: Vec<&Card> = order.iter()
                .filter_map(|id| cards.get(id))
                .filter(|c| c.status == col_status)
                .collect();

            if col_cards.is_empty() && col_status != CardStatus::Todo {
                continue;
            }

            output.push_str(&format!("\n  {col_name} ({})\n", col_cards.len()));
            output.push_str(&format!("  {}\n", "-".repeat(50)));

            for card in col_cards {
                let priority_icon = match card.priority {
                    Priority::Critical => "🔴",
                    Priority::High => "🟡",
                    Priority::Medium => "🟢",
                    Priority::Low => "⚪",
                };
                let deps = if card.dependencies.is_empty() {
                    String::new()
                } else {
                    format!(" [dep: {}]", card.dependencies.join(", "))
                };
                output.push_str(&format!("    {priority_icon} {} — {}{deps}\n",
                    &card.title[..card.title.len().min(50)],
                    &card.description[..card.description.len().min(60)],
                ));
            }
        }

        output
    }
}
