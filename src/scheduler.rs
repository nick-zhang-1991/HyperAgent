//! Task scheduler — schedule recurring agent runs using cron expressions
//!
//! # Usage
//! ```bash
//! hyper schedule add "0 9 * * *" "run tests and report"   # Daily at 9 AM
//! hyper schedule list                                       # List all jobs
//! hyper schedule remove <id>                                # Remove a job
//! hyper schedule run <id>                                   # Run a job now
//! hyper daemon                                              # Start scheduler daemon
//! ```

use anyhow::{Context, Result};
use cron::Schedule;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::str::FromStr;

/// A scheduled job
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledJob {
    pub id: String,
    pub cron_expr: String,
    pub prompt: String,
    pub mode: String,
    pub enabled: bool,
    pub last_run: Option<String>,
    pub next_run: Option<String>,
    pub run_count: u32,
}

/// Scheduler state
pub struct Scheduler {
    dir: PathBuf,
}

impl Scheduler {
    /// Create a new scheduler for the given project root
    pub fn new(root: &PathBuf) -> Self {
        let dir = root.join(".hyper").join("schedule");
        Self { dir }
    }

    /// Ensure the schedule directory exists
    fn ensure_dir(&self) -> Result<()> {
        std::fs::create_dir_all(&self.dir)
            .with_context(|| format!("Failed to create schedule dir: {}", self.dir.display()))
    }

    /// Add a new scheduled job
    pub fn add(&self, cron_expr: &str, prompt: &str, mode: &str) -> Result<ScheduledJob> {
        // Validate cron expression
        Schedule::from_str(cron_expr)
            .map_err(|e| anyhow::anyhow!("Invalid cron expression '{cron_expr}': {e}"))?;

        self.ensure_dir()?;

        let id = uuid::Uuid::new_v4().to_string()[..8].to_string();
        let now = chrono::Utc::now();

        let job = ScheduledJob {
            id: id.clone(),
            cron_expr: cron_expr.to_string(),
            prompt: prompt.to_string(),
            mode: mode.to_string(),
            enabled: true,
            last_run: None,
            next_run: Some(self.next_occurrence(cron_expr, &now)?.to_rfc3339()),
            run_count: 0,
        };

        let path = self.dir.join(format!("{id}.json"));
        let json = serde_json::to_string_pretty(&job)?;
        std::fs::write(&path, json)?;

        println!("   ✅ Scheduled job '{id}': {cron_expr} → \"{}\"", &prompt[..prompt.len().min(60)]);
        if let Some(ref next) = job.next_run {
            println!("   ⏰ Next run: {next}");
        }

        Ok(job)
    }

    /// List all scheduled jobs
    pub fn list(&self) -> Result<Vec<ScheduledJob>> {
        self.ensure_dir()?;
        let mut jobs = Vec::new();

        let entries = std::fs::read_dir(&self.dir)?;
        for entry in entries {
            let entry = entry?;
            if entry.path().extension().map(|e| e == "json").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(entry.path()) {
                    if let Ok(job) = serde_json::from_str::<ScheduledJob>(&content) {
                        jobs.push(job);
                    }
                }
            }
        }

        jobs.sort_by(|a, b| a.next_run.as_ref().unwrap_or(&String::new()).cmp(b.next_run.as_ref().unwrap_or(&String::new())));
        Ok(jobs)
    }

    /// Remove a scheduled job by ID
    pub fn remove(&self, id: &str) -> Result<()> {
        self.ensure_dir()?;
        let path = self.dir.join(format!("{id}.json"));
        if path.exists() {
            std::fs::remove_file(&path)?;
            println!("   🗑️  Removed schedule: {id}");
            Ok(())
        } else {
            anyhow::bail!("Schedule '{id}' not found");
        }
    }

    /// Get a specific job
    pub fn get(&self, id: &str) -> Result<ScheduledJob> {
        let path = self.dir.join(format!("{id}.json"));
        let content = std::fs::read_to_string(&path)
            .with_context(|| format!("Schedule '{id}' not found"))?;
        let job = serde_json::from_str(&content)?;
        Ok(job)
    }

    /// Get jobs that are due to run
    pub fn due_jobs(&self) -> Result<Vec<ScheduledJob>> {
        let now = chrono::Utc::now();
        let jobs = self.list()?;
        Ok(jobs
            .into_iter()
            .filter(|j| j.enabled)
            .filter(|j| {
                j.next_run.as_ref().map_or(false, |next| {
                    chrono::DateTime::parse_from_rfc3339(next)
                        .map(|t| t <= now)
                        .unwrap_or(false)
                })
            })
            .collect())
    }

    /// Mark a job as run and update next_run
    pub fn mark_run(&self, job: &ScheduledJob) -> Result<ScheduledJob> {
        let mut updated = job.clone();
        let now = chrono::Utc::now();
        updated.last_run = Some(now.to_rfc3339());
        updated.run_count += 1;
        updated.next_run = Some(self.next_occurrence(&job.cron_expr, &now)?.to_rfc3339());

        let path = self.dir.join(format!("{}.json", job.id));
        let json = serde_json::to_string_pretty(&updated)?;
        std::fs::write(&path, json)?;

        Ok(updated)
    }

    /// Calculate next occurrence from a cron expression
    fn next_occurrence(&self, cron_expr: &str, from: &chrono::DateTime<chrono::Utc>) -> Result<chrono::DateTime<chrono::Utc>> {
        let schedule = Schedule::from_str(cron_expr)
            .map_err(|e| anyhow::anyhow!("Invalid cron: {e}"))?;

        schedule.after(from).next()
            .ok_or_else(|| anyhow::anyhow!("No future occurrence for cron: {cron_expr}"))
    }
}

/// Display scheduled jobs in a table
pub fn display_jobs(jobs: &[ScheduledJob]) {
    if jobs.is_empty() {
        println!("   📅 No scheduled jobs.");
        println!("   Add one: hyper schedule add \"0 9 * * *\" \"your task\"");
        return;
    }

    println!("   {:<10} {:<20} {:<20} {:<10} {:<8} {:<8}", "ID", "Cron", "Next Run", "Mode", "Runs", "Enabled");
    println!("   {:-<10} {:-<20} {:-<20} {:-<10} {:-<8} {:-<8}", "", "", "", "", "", "");
    for job in jobs {
        let next = job.next_run.as_deref().unwrap_or("-");
        let next_short = if next.len() > 19 { &next[..19] } else { next };
        let enabled = if job.enabled { "✅" } else { "⬜" };
        println!("   {:<10} {:<20} {:<20} {:<10} {:<8} {:<8}",
            job.id, job.cron_expr, next_short, job.mode, job.run_count, enabled);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_add_and_list() {
        let dir = tempdir().unwrap();
        let sched = Scheduler::new(&dir.path().to_path_buf());
        let job = sched.add("0 0 0 * * *", "test task", "ask").unwrap();
        assert_eq!(job.prompt, "test task");
        assert_eq!(job.mode, "ask");

        let jobs = sched.list().unwrap();
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].id, job.id);
    }

    #[test]
    fn test_remove() {
        let dir = tempdir().unwrap();
        let sched = Scheduler::new(&dir.path().to_path_buf());
        let job = sched.add("0 0 0 * * *", "test", "code").unwrap();
        assert!(sched.list().unwrap().len() == 1);
        sched.remove(&job.id).unwrap();
        assert!(sched.list().unwrap().is_empty());
    }

    #[test]
    fn test_invalid_cron() {
        let dir = tempdir().unwrap();
        let sched = Scheduler::new(&dir.path().to_path_buf());
        assert!(sched.add("invalid", "test", "ask").is_err());
    }

    #[test]
    fn test_due_jobs() {
        let dir = tempdir().unwrap();
        let sched = Scheduler::new(&dir.path().to_path_buf());
        let job = sched.add("0 0 0 * * *", "daily midnight", "code").unwrap();
        // Job should exist in list
        let jobs = sched.list().unwrap();
        assert!(!jobs.is_empty());
        assert_eq!(jobs[0].id, job.id);
        // Next run should be in the future
        let next = &job.next_run;
        assert!(next.is_some());
    }

    #[test]
    fn test_mark_run() {
        let dir = tempdir().unwrap();
        let sched = Scheduler::new(&dir.path().to_path_buf());
        let job = sched.add("0 0 0 * * *", "daily", "ask").unwrap();
        assert_eq!(job.run_count, 0);

        let updated = sched.mark_run(&job).unwrap();
        assert_eq!(updated.run_count, 1);
        assert!(updated.last_run.is_some());
        assert!(updated.next_run.is_some());
    }
}
