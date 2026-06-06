//! Team Workspaces — Shared agent memory, skills, and rules for teams.
//!
//! For 100M users, team collaboration is a key monetization driver (Team tier).
//!
//! Architecture:
//! - Local workspace config in `.hyper/team.toml`
//! - Shared memory database (SQLite with file locking for concurrency)
//! - Shared skills directory
//! - Role-based access: admin, member, viewer
//! - Optional cloud sync backend
//!
//! Commands:
//!   hyper team init <name>        — Create a team workspace
//!   hyper team join <invite-code> — Join existing team
//!   hyper team members            — List team members
//!   hyper team invite <email>     — Generate invite code

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamConfig {
    pub name: String,
    pub team_id: String,
    pub created_at: u64,
    pub members: Vec<TeamMember>,
    pub shared_memory: bool,
    pub shared_skills: bool,
    pub shared_rules: bool,
    pub cloud_sync_enabled: bool,
    pub cloud_endpoint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub email: String,
    pub role: MemberRole,
    pub joined_at: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MemberRole {
    Admin,
    Member,
    Viewer,
}

impl MemberRole {
    pub fn can_edit(&self) -> bool {
        matches!(self, MemberRole::Admin | MemberRole::Member)
    }

    pub fn can_manage(&self) -> bool {
        matches!(self, MemberRole::Admin)
    }
}

impl TeamConfig {
    pub fn load() -> Result<Option<Self>> {
        let path = team_config_path();
        if path.exists() {
            let json = std::fs::read_to_string(&path)?;
            Ok(Some(serde_json::from_str(&json)?))
        } else {
            Ok(None)
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = team_config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    pub fn init(name: &str, admin_email: &str) -> Result<Self> {
        if Self::load()?.is_some() {
            bail!("Team already initialized in this project. Use 'hyper team leave' first.");
        }

        let team = TeamConfig {
            name: name.to_string(),
            team_id: uuid::Uuid::new_v4().to_string(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            members: vec![TeamMember {
                email: admin_email.to_string(),
                role: MemberRole::Admin,
                joined_at: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs(),
            }],
            shared_memory: true,
            shared_skills: true,
            shared_rules: true,
            cloud_sync_enabled: false,
            cloud_endpoint: None,
        };

        team.save()?;
        println!();
        println!("  \x1b[1;32m✅ Team '{}' created!\x1b[0m", name);
        println!();
        println!("  Team ID: \x1b[36m{}\x1b[0m", team.team_id);
        println!("  Members: 1 (you, as Admin)");
        println!();
        println!("  \x1b[1mNext steps:\x1b[0m");
        println!("    hyper team invite <email>     # Invite members");
        println!("    hyper team members            # List members");
        println!("    hyper team share-memory       # Toggle shared memory");
        println!();

        Ok(team)
    }

    pub fn add_member(&mut self, email: &str, role: MemberRole) -> Result<()> {
        if self.members.iter().any(|m| m.email == email) {
            bail!("{} is already a team member", email);
        }
        self.members.push(TeamMember {
            email: email.to_string(),
            role: role.clone(),
            joined_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        });
        self.save()?;
        println!("  ✅ {} added as {:?}", email, role);
        Ok(())
    }

    pub fn remove_member(&mut self, email: &str) -> Result<()> {
        let len_before = self.members.len();
        self.members.retain(|m| m.email != email);
        if self.members.len() == len_before {
            bail!("{} is not a team member", email);
        }
        self.save()?;
        println!("  ✅ {} removed from team", email);
        Ok(())
    }

    pub fn print_members(&self) {
        println!();
        println!("  \x1b[1;36m👥 Team: {}\x1b[0m", self.name);
        println!("  {}", "─".repeat(50));
        for member in &self.members {
            let role_icon = match member.role {
                MemberRole::Admin => "👑",
                MemberRole::Member => "👤",
                MemberRole::Viewer => "👁",
            };
            println!(
                "  {}  {:30}  {:?}",
                role_icon, member.email, member.role
            );
        }
        println!();
        println!("  Shared memory: {}", if self.shared_memory { "✅" } else { "❌" });
        println!("  Shared skills: {}", if self.shared_skills { "✅" } else { "❌" });
        println!("  Cloud sync:    {}", if self.cloud_sync_enabled { "✅" } else { "❌" });
        println!();
    }
}

fn team_config_path() -> PathBuf {
    PathBuf::from(".hyper").join("team.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_member_roles() {
        assert!(MemberRole::Admin.can_manage());
        assert!(MemberRole::Admin.can_edit());
        assert!(MemberRole::Member.can_edit());
        assert!(!MemberRole::Member.can_manage());
        assert!(!MemberRole::Viewer.can_edit());
        assert!(!MemberRole::Viewer.can_manage());
    }

    #[test]
    fn test_team_add_member() {
        let mut team = TeamConfig {
            name: "Test Team".into(),
            team_id: "test-1".into(),
            created_at: 0,
            members: vec![],
            shared_memory: true,
            shared_skills: true,
            shared_rules: true,
            cloud_sync_enabled: false,
            cloud_endpoint: None,
        };
        assert!(team.add_member("alice@example.com", MemberRole::Member).is_ok());
        assert!(team.add_member("alice@example.com", MemberRole::Member).is_err()); // duplicate
        assert_eq!(team.members.len(), 1);
    }
}
