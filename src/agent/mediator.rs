//! Task mediator — orchestrates the agent pipeline lifecycle with typed errors
//! and session-aware task coordination.
//!
//! The TaskCoordinator wraps Orchestrator::run() with:
//! - Typed error handling (AgentError enum)
//! - Phase tracking (Planning → Coding → Reviewing → Applying → Complete)
//! - Session save/resume integration
//! - Pipeline status reporting

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::hooks::HookRegistry;
use crate::index::HyperIndex;
use crate::knowledge::KnowledgeBase;
use crate::llm::{LlmProvider, ProviderPool};
use crate::memory::MemoryManager;
use crate::mcp::McpRegistry;
use crate::modes::ModeRegistry;
use crate::plugin::PluginManager;
use crate::session::Session;

use super::orchestrator::{Orchestrator, RunResult};

// ═══════════════════════════════════════════════
// Error types
// ═══════════════════════════════════════════════

/// Typed error for agent pipeline operations
#[derive(Debug, Clone, thiserror::Error)]
pub enum AgentError {
    #[error("LLM provider error: {0}")]
    Provider(String),

    #[error("Index building error: {0}")]
    Index(String),

    #[error("Tool execution failed: {0}")]
    Tool(String),

    #[error("Session persistence error: {0}")]
    Session(String),

    #[error("Plugin error: {0}")]
    Plugin(String),

    #[error("Task cancelled: {0}")]
    Cancelled(String),

    #[error("Internal error: {0}")]
    Internal(String),
}

impl From<anyhow::Error> for AgentError {
    fn from(e: anyhow::Error) -> Self {
        AgentError::Internal(e.to_string())
    }
}

// ═══════════════════════════════════════════════
// Phase tracking
// ═══════════════════════════════════════════════

/// Lifecycle phase of a coordinated task
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum TaskPhase {
    /// Planning the task decomposition
    Planning,
    /// Executing code changes
    Coding,
    /// Reviewing generated changes
    Reviewing,
    /// Applying approved changes
    Applying,
    /// All phases completed
    Complete,
    /// Task failed at a given phase
    Failed(String),
}

impl std::fmt::Display for TaskPhase {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskPhase::Planning => write!(f, "planning"),
            TaskPhase::Coding => write!(f, "coding"),
            TaskPhase::Reviewing => write!(f, "reviewing"),
            TaskPhase::Applying => write!(f, "applying"),
            TaskPhase::Complete => write!(f, "complete"),
            TaskPhase::Failed(reason) => write!(f, "failed: {reason}"),
        }
    }
}

/// Result of a mediated task execution
#[derive(Debug, Clone)]
pub struct TaskResult {
    /// The underlying orchestrator result
    pub inner: RunResult,
    /// Final phase after execution
    pub phase: TaskPhase,
    /// Total wall-clock time
    pub total_elapsed: Duration,
    /// Session ID if session was active
    pub session_id: Option<String>,
    /// Number of tool calls made
    pub tool_calls: usize,
    pub pipeline: PipelineTracker,
}

// ═══════════════════════════════════════════════
// Pipeline tracking — per-phase metrics & events
// ═══════════════════════════════════════════════

/// Per-phase execution metrics
#[derive(Debug, Clone, Default)]
pub struct PhaseMetrics {
    /// Wall-clock time spent in this phase
    pub duration: Duration,
    /// LLM tokens consumed
    pub tokens_used: usize,
    /// Number of tool calls made
    pub tool_calls: usize,
    /// Files modified in this phase
    pub files_modified: usize,
    /// Estimated USD cost
    pub cost_estimate: f64,
}

impl PhaseMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a completed phase with metrics from RunResult
    pub fn from_run_result(r: &RunResult, elapsed: Duration) -> Self {
        Self {
            duration: elapsed,
            tokens_used: r.tokens_used,
            tool_calls: 0, // RunResult doesn't expose tool call count
            files_modified: r.files_modified,
            cost_estimate: r.cost_estimate,
        }
    }
}

/// Events that fire during agent pipeline execution
#[derive(Debug, Clone)]
pub enum PipelineEvent {
    /// A phase has started
    PhaseStarted(TaskPhase),
    /// A phase has completed with metrics
    PhaseCompleted(TaskPhase, PhaseMetrics),
    /// The pipeline encountered an error
    Error(TaskPhase, AgentError),
    /// The pipeline completed successfully
    Completed(TaskResult),
}

/// Tracks the full pipeline lifecycle with per-phase metrics
#[derive(Debug, Clone)]
pub struct PipelineTracker {
    phases: Vec<(TaskPhase, PhaseMetrics)>,
    start_time: Instant,
    phase_start: Instant,
    current: TaskPhase,
    pub total_tokens: usize,
    pub total_tool_calls: usize,
    pub total_files_modified: usize,
    pub total_cost: f64,
    events: Vec<PipelineEvent>,
}

impl PipelineTracker {
    pub fn new() -> Self {
        Self {
            phases: Vec::with_capacity(5),
            start_time: Instant::now(),
            phase_start: Instant::now(),
            current: TaskPhase::Planning,
            total_tokens: 0,
            total_tool_calls: 0,
            total_files_modified: 0,
            total_cost: 0.0,
            events: Vec::new(),
        }
    }

    /// Transition to a new pipeline phase
    pub fn transition(&mut self, phase: TaskPhase) -> PipelineEvent {
        let elapsed = self.phase_start.elapsed();
        let metrics = PhaseMetrics {
            duration: elapsed,
            ..Default::default()
        };
        self.phases.push((self.current.clone(), metrics.clone()));
        self.events.push(PipelineEvent::PhaseCompleted(self.current.clone(), metrics));
        
        self.current = phase;
        self.phase_start = Instant::now();
        let event = PipelineEvent::PhaseStarted(self.current.clone());
        self.events.push(event.clone());
        event
    }

    /// Record metrics for the current phase (called when phase completes)
    pub fn complete_phase(&mut self, r: &RunResult) -> PipelineEvent {
        let elapsed = self.phase_start.elapsed();
        let metrics = PhaseMetrics::from_run_result(r, elapsed);
        
        self.total_tokens += metrics.tokens_used;
        self.total_files_modified += metrics.files_modified;
        self.total_cost += metrics.cost_estimate;
        
        self.phases.push((self.current.clone(), metrics.clone()));
        let event = PipelineEvent::PhaseCompleted(self.current.clone(), metrics);
        self.events.push(event.clone());
        event
    }

    /// Get metrics for a specific phase
    pub fn phase_metrics(&self, phase: &TaskPhase) -> Option<&PhaseMetrics> {
        self.phases.iter()
            .find(|(p, _)| p == phase)
            .map(|(_, m)| m)
    }

    /// Total elapsed time since pipeline start
    pub fn total_elapsed(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Number of phases completed so far
    pub fn phase_count(&self) -> usize {
        self.phases.len()
    }

    /// All pipeline events collected during execution
    pub fn events(&self) -> &[PipelineEvent] {
        &self.events
    }

    /// Display a summary of the pipeline execution
    pub fn summary(&self) -> String {
        let elapsed = self.total_elapsed();
        let secs = elapsed.as_secs_f64();
        let mut lines = Vec::new();
        lines.push(format!("Pipeline: {} phases in {:.1}s", self.phases.len(), secs));
        lines.push(format!("  Tokens: {} | Files: {} | Cost: ${:.4}", 
            self.total_tokens, self.total_files_modified, self.total_cost));
        for (phase, metrics) in &self.phases {
            let d = metrics.duration.as_secs_f64();
            lines.push(format!("  {phase}: {d:.1}s, {t}tok, {f}files, ${c:.4}",
                t = metrics.tokens_used, f = metrics.files_modified, c = metrics.cost_estimate));
        }
        lines.join("\n")
    }
}

impl Default for PipelineTracker {
    fn default() -> Self {
        Self::new()
    }
}

// ═══════════════════════════════════════════════
// TaskCoordinator
// ═══════════════════════════════════════════════

/// Parse a tool result string into a typed Result.
/// Tool results with "Error: " prefix are treated as failures.
pub fn parse_tool_result(result: &str, _tool_name: &str) -> Result<String, AgentError> {
    if result.starts_with("Error: ") {
        Err(AgentError::Tool(result[7..].to_string()))
    } else if result.starts_with("⚠️") || result.starts_with("❌") {
        // Emoji-prefixed errors (from some built-in tools)
        Err(AgentError::Tool(result.to_string()))
    } else {
        Ok(result.to_string())
    }
}

/// Coordinates the full agent pipeline — wraps Orchestrator with 
/// typed errors, phase tracking, and session integration.
pub struct TaskCoordinator {
    orchestrator: Option<Orchestrator>,
    session: Option<Session>,
    task_id: String,
    current_phase: TaskPhase,
    start_time: Option<Instant>,

    // Stored builder state (deferred until init())
    root: PathBuf,
    parallel_agents: usize,
    confirm: bool,
    mode: Option<String>,
    project_context: Option<String>,
    conversation_history: Vec<(String, String)>,
    memory: Option<MemoryManager>,
    hooks: Option<HookRegistry>,
    mcp: Option<McpRegistry>,
    mode_registry: Option<ModeRegistry>,
    pending_image: Option<String>,
    knowledge_base: Option<KnowledgeBase>,
    sandbox_enabled: bool,
    plugin_manager: Option<PluginManager>,
    provider_pool: Option<ProviderPool>,
    plan_provider: Option<LlmProvider>,
    review_provider: Option<LlmProvider>,
    tracker: PipelineTracker,
}

impl TaskCoordinator {
    /// Create a new task coordinator. Call `.init()` with an index and provider
    /// to construct the inner orchestrator.
    pub fn new(root: PathBuf, parallel_agents: usize, confirm: bool) -> Self {
        Self {
            orchestrator: None,
            session: None,
            task_id: format!("task-{}", std::process::id()),
            current_phase: TaskPhase::Planning,
            start_time: None,
            root,
            parallel_agents,
            confirm,
            mode: None,
            project_context: None,
            conversation_history: Vec::new(),
            memory: None,
            hooks: None,
            mcp: None,
            mode_registry: None,
            pending_image: None,
            knowledge_base: None,
            sandbox_enabled: false,
            plugin_manager: None,
            provider_pool: None,
            plan_provider: None,
            review_provider: None,
            tracker: PipelineTracker::new(),
        }
    }

    // ── Builder methods ──────────────────────────────────────────

    pub fn with_session(mut self, session: Session) -> Self {
        self.session = Some(session);
        self
    }

    pub fn with_task_id(mut self, id: impl Into<String>) -> Self {
        self.task_id = id.into();
        self
    }

    pub fn with_mode(mut self, mode: impl Into<String>) -> Self {
        self.mode = Some(mode.into());
        self
    }

    pub fn with_project_context(mut self, ctx: String) -> Self {
        self.project_context = Some(ctx);
        self
    }

    pub fn with_conversation_history(mut self, history: Vec<(String, String)>) -> Self {
        self.conversation_history = history;
        self
    }

    pub fn with_memory(mut self, memory: MemoryManager) -> Self {
        self.memory = Some(memory);
        self
    }

    pub fn with_hooks(mut self, hooks: HookRegistry) -> Self {
        self.hooks = Some(hooks);
        self
    }

    pub fn with_mcp(mut self, mcp: McpRegistry) -> Self {
        self.mcp = Some(mcp);
        self
    }

    pub fn with_mode_registry(mut self, registry: ModeRegistry) -> Self {
        self.mode_registry = Some(registry);
        self
    }

    pub fn with_image(mut self, image: String) -> Self {
        self.pending_image = Some(image);
        self
    }

    pub fn with_knowledge_base(mut self, kb: KnowledgeBase) -> Self {
        self.knowledge_base = Some(kb);
        self
    }

    pub fn with_sandbox(mut self) -> Self {
        self.sandbox_enabled = true;
        self
    }

    pub fn with_plugins(mut self, pm: PluginManager) -> Self {
        self.plugin_manager = Some(pm);
        self
    }

    pub fn with_provider_pool(mut self, pool: ProviderPool) -> Self {
        self.provider_pool = Some(pool);
        self
    }

    pub fn with_plan_provider(mut self, provider: LlmProvider) -> Self {
        self.plan_provider = Some(provider);
        self
    }

    pub fn with_review_provider(mut self, provider: LlmProvider) -> Self {
        self.review_provider = Some(provider);
        self
    }

    /// Build the inner orchestrator from deferred builder state.
    /// Must be called before `run()`.
    pub fn init(&mut self, index: HyperIndex, provider: LlmProvider) -> Result<()> {
        let mut orch = Orchestrator::new(
            index,
            provider,
            self.root.clone(),
            self.parallel_agents,
            self.confirm,
        );

        if let Some(mode) = &self.mode {
            orch = orch.with_mode(mode);
        }
        if let Some(ctx) = &self.project_context {
            orch = orch.with_project_context(ctx.clone());
        }
        if !self.conversation_history.is_empty() {
            orch = orch.with_conversation_history(std::mem::take(&mut self.conversation_history));
        }
        if let Some(m) = self.memory.take() {
            orch = orch.with_memory(m);
        }
        if let Some(h) = self.hooks.take() {
            orch = orch.with_hooks(h);
        }
        if let Some(m) = self.mcp.take() {
            orch = orch.with_mcp(m);
        }
        if let Some(r) = self.mode_registry.take() {
            orch = orch.with_mode_registry(r);
        }
        if let Some(img) = &self.pending_image {
            orch.with_image(img.clone()); // with_image returns &mut Self, modifies in place
        }
        if let Some(kb) = self.knowledge_base.take() {
            orch = orch.with_knowledge_base(kb);
        }
        if let Some(pm) = self.plugin_manager.take() {
            orch = orch.with_plugins(pm);
        }
        if let Some(pool) = self.provider_pool.take() {
            orch = orch.with_provider_pool(pool);
        }
        if let Some(pp) = self.plan_provider.take() {
            orch = orch.with_plan_provider(pp);
        }
        if let Some(rp) = self.review_provider.take() {
            orch = orch.with_review_provider(rp);
        }
        if self.sandbox_enabled {
            orch = orch.with_sandbox();
        }

        self.orchestrator = Some(orch);
        Ok(())
    }

    // ── Execution ─────────────────────────────────────────────────

    /// Get a reference to the inner orchestrator (for advanced config).
    pub fn orchestrator(&self) -> Option<&Orchestrator> {
        self.orchestrator.as_ref()
    }

    /// Get a mutable reference to the inner orchestrator.
    pub fn orchestrator_mut(&mut self) -> Option<&mut Orchestrator> {
        self.orchestrator.as_mut()
    }

    /// Current task phase
    pub fn phase(&self) -> &TaskPhase {
        &self.current_phase
    }

    /// Task ID
    pub fn task_id(&self) -> &str {
        &self.task_id
    }

    /// Run the full agent pipeline for a given prompt.
    ///
    /// Returns a `TaskResult` with phase tracking — callers can inspect
    /// the phase to determine if the task completed or failed and at which stage.
    pub async fn run(&mut self, prompt: &str) -> TaskResult {
        let start = Instant::now();
        self.start_time = Some(start);
        self.current_phase = TaskPhase::Planning;

        let orch = match &mut self.orchestrator {
            Some(o) => o,
            None => {
                let elapsed = start.elapsed();
                return TaskResult {
                    inner: RunResult::default(),
                    phase: TaskPhase::Failed("TaskCoordinator not initialized — call .init() first".into()),
                    total_elapsed: elapsed,
                    session_id: self.session.as_ref().map(|s| s.id.clone()),
                    tool_calls: 0,
                    pipeline: PipelineTracker::new(),
                };
            }
        };

        // Phase progression: Planning → Coding → Reviewing (handled internally by orchestrator)
        self.current_phase = TaskPhase::Coding;

        let inner_result = match orch.run(prompt).await {
            Ok(r) => r,
            Err(e) => {
                let elapsed = start.elapsed();
                return TaskResult {
                    inner: RunResult::default(),
                    phase: TaskPhase::Failed(format!("execution failed: {e}")),
                    total_elapsed: elapsed,
                    session_id: self.session.as_ref().map(|s| s.id.clone()),
                    tool_calls: 0,
            pipeline: PipelineTracker::new(),
                };
            }
        };

        self.tracker.complete_phase(&inner_result);
        self.current_phase = TaskPhase::Complete;
        self.tracker.transition(TaskPhase::Complete);

        // Save session if session manager is active
        let session_id = self.session.as_ref().map(|s| s.id.clone()).or(None);

        TaskResult {
            inner: inner_result,
            phase: TaskPhase::Complete,
            total_elapsed: start.elapsed(),
            session_id,
            tool_calls: 0, // orchestrator doesn't expose tool call count yet
            pipeline: PipelineTracker::new(),
        }
    }
}

// ═══════════════════════════════════════════════
// Tests
// ═══════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_error_display() {
        let err = AgentError::Provider("rate limited".into());
        assert_eq!(err.to_string(), "LLM provider error: rate limited");

        let err = AgentError::Cancelled("user interrupt".into());
        assert_eq!(err.to_string(), "Task cancelled: user interrupt");
    }

    #[test]
    fn test_task_phase_display() {
        assert_eq!(TaskPhase::Planning.to_string(), "planning");
        assert_eq!(TaskPhase::Complete.to_string(), "complete");
        assert_eq!(
            TaskPhase::Failed("timeout".into()).to_string(),
            "failed: timeout"
        );
    }

    #[test]
    fn test_task_coordinator_new() {
        let coord = TaskCoordinator::new("/tmp".into(), 2, true);
        assert_eq!(coord.parallel_agents, 2);
        assert_eq!(coord.current_phase, TaskPhase::Planning);
        assert!(coord.orchestrator.is_none());
    }

    #[test]
    fn test_task_coordinator_builder() {
        let coord = TaskCoordinator::new("/tmp".into(), 2, true)
            .with_mode("ask")
            .with_task_id("test-1")
            .with_sandbox();

        assert_eq!(coord.mode, Some("ask".into()));
        assert_eq!(coord.task_id, "test-1");
        assert!(coord.sandbox_enabled);
    }

    #[test]
    fn test_pipeline_tracker_basic() {
        let mut pt = PipelineTracker::new();
        assert_eq!(pt.phase_count(), 0);
        
        pt.transition(TaskPhase::Coding);
        assert_eq!(pt.phase_count(), 1);
        
        // Quick sleep to ensure non-zero duration
        std::thread::sleep(std::time::Duration::from_millis(1));
        pt.transition(TaskPhase::Reviewing);
        assert_eq!(pt.phase_count(), 2);
        
        let elapsed = pt.total_elapsed();
        assert!(elapsed.as_secs_f64() > 0.0);
    }

    #[test]
    fn test_pipeline_tracker_summary() {
        let mut pt = PipelineTracker::new();
        pt.transition(TaskPhase::Coding);
        pt.transition(TaskPhase::Complete);
        
        let summary = pt.summary();
        assert!(summary.contains("Pipeline:"));
        assert!(summary.contains("phases"));
    }

    #[test]
    fn test_pipeline_tracker_events() {
        let mut pt = PipelineTracker::new();
        pt.transition(TaskPhase::Coding);
        pt.transition(TaskPhase::Complete);
        
        let events = pt.events();
        assert!(events.len() >= 2);
        // First event should be PhaseCompleted(Planning) and start of Coding
    }

    #[test]
    fn test_parse_tool_result_success() {
        let result = parse_tool_result("Search results: found 5 items", "web_search");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "Search results: found 5 items");
    }

    #[test]
    fn test_parse_tool_result_error() {
        let result = parse_tool_result("Error: web search failed: timeout", "web_search");
        assert!(result.is_err());
        match result {
            Err(AgentError::Tool(msg)) => assert_eq!(msg, "web search failed: timeout"),
            _ => panic!("Expected Tool error"),
        }
    }

    #[test]
    fn test_parse_tool_result_emoji_error() {
        let result = parse_tool_result("⚠️ Memory not found", "memory_search");
        assert!(result.is_err());
    }

    #[test]
    fn test_run_without_init_returns_failed() {
        // Run without calling init first — should return Failed phase
        let mut coord = TaskCoordinator::new("/tmp".into(), 2, true);
        let future = coord.run("test prompt");
        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(future);
        assert_eq!(
            result.phase,
            TaskPhase::Failed(
                "TaskCoordinator not initialized — call .init() first".into()
            )
        );
    }
}
