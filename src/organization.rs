//! Organization & Enterprise SSO — team management, OIDC, RBAC
//!
//! Local file-based organization management without external server.
//! Supports:
//! - Organization profiles (.hyper/organization.toml)
//! - Team membership with roles
//! - OIDC provider configuration
//! - RBAC: admin / member / viewer
//!
//! Usage:
//!   hyper org init "My Company"          # Create org
//!   hyper org status                      # Show org info
//!   hyper org team add bob@company.com"   # Add team member
//!   hyper org team list                   # List team members
//!   hyper org invite "bob@company.com"    # Generate invite code

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Role within an organization
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OrgRole {
    Admin,
    Member,
    Viewer,
}

impl std::fmt::Display for OrgRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrgRole::Admin => write!(f, "admin"),
            OrgRole::Member => write!(f, "member"),
            OrgRole::Viewer => write!(f, "viewer"),
        }
    }
}

/// A team member
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TeamMember {
    pub email: String,
    pub name: Option<String>,
    pub role: OrgRole,
    pub added_at: String,
    pub last_active: Option<String>,
}

/// OIDC provider configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OidcProvider {
    pub name: String,
    pub issuer_url: String,
    pub client_id: String,
    pub client_secret: Option<String>,
    pub scopes: Vec<String>,
}

impl Default for OidcProvider {
    fn default() -> Self {
        Self {
            name: String::new(),
            issuer_url: String::new(),
            client_id: String::new(),
            client_secret: None,
            scopes: vec!["openid".into(), "profile".into(), "email".into()],
        }
    }
}

/// Organization configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Organization {
    pub name: String,
    pub created_at: String,
    pub owner: String,
    pub members: Vec<TeamMember>,
    pub oidc_providers: Vec<OidcProvider>,
    pub policies: OrgPolicies,
}

/// Organization security policies
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrgPolicies {
    pub require_mfa: bool,
    pub require_approval_for_commands: Vec<String>,
    pub max_budget_per_member: f64,
    pub audit_logging: bool,
    pub allowed_domains: Vec<String>,
}

impl Default for OrgPolicies {
    fn default() -> Self {
        Self {
            require_mfa: false,
            require_approval_for_commands: vec!["deploy".into(), "delete".into()],
            max_budget_per_member: 50.0,
            audit_logging: true,
            allowed_domains: vec![],
        }
    }
}

/// Organization manager
pub struct OrgManager {
    org_path: PathBuf,
    organization: Option<Organization>,
}

impl OrgManager {
    /// Create a manager for the given project
    pub fn new(project_root: &Path) -> Self {
        let org_path = project_root.join(".hyper").join("organization.toml");
        let organization = Self::load(&org_path);
        Self { org_path, organization }
    }

    /// Load organization from file
    fn load(path: &Path) -> Option<Organization> {
        if !path.exists() {
            return None;
        }
        let content = std::fs::read_to_string(path).ok()?;
        toml::from_str(&content).ok()
    }

    /// Save organization to file
    fn save(&self) -> anyhow::Result<()> {
        if let Some(ref org) = self.organization {
            if let Some(parent) = self.org_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let content = toml::to_string_pretty(org)?;
            std::fs::write(&self.org_path, content)?;
        }
        Ok(())
    }

    /// Initialize a new organization
    pub fn init(&mut self, name: &str, owner: &str) -> anyhow::Result<String> {
        if self.organization.is_some() {
            anyhow::bail!("Organization already exists. Use `org status` to view.");
        }

        let org = Organization {
            name: name.to_string(),
            created_at: chrono::Utc::now().to_rfc3339(),
            owner: owner.to_string(),
            members: vec![TeamMember {
                email: owner.to_string(),
                name: Some("Owner".into()),
                role: OrgRole::Admin,
                added_at: chrono::Utc::now().to_rfc3339(),
                last_active: Some(chrono::Utc::now().to_rfc3339()),
            }],
            oidc_providers: vec![],
            policies: OrgPolicies::default(),
        };

        self.organization = Some(org);
        self.save()?;
        Ok(format!("✅ Organization '{name}' created. Owner: {owner}"))
    }

    /// Get organization info
    pub fn info(&self) -> String {
        match &self.organization {
            Some(org) => {
                let mut output = format!(
                    "   🏢 Organization: {}\n",
                    org.name
                );
                output.push_str(&format!("   👤 Owner: {}\n", org.owner));
                output.push_str(&format!("   📅 Created: {}\n", &org.created_at[..10]));
                output.push_str(&format!("   👥 Members: {}\n", org.members.len()));
                output.push_str(&format!("   🔐 OIDC: {} provider(s)\n", org.oidc_providers.len()));
                output.push_str(&format!("   💰 Budget/member: ${:.2}\n", org.policies.max_budget_per_member));
                output.push_str(&format!("   📋 Audit: {}\n", if org.policies.audit_logging { "enabled" } else { "disabled" }));
                output
            }
            None => "   No organization configured. Use `hyper org init \"Company Name\" --owner you@email.com`".to_string(),
        }
    }

    /// List team members
    pub fn list_members(&self) -> String {
        match &self.organization {
            Some(org) => {
                if org.members.is_empty() {
                    return "   No team members.".to_string();
                }
                let mut output = String::from("   👥 Team Members\n");
                for member in &org.members {
                    output.push_str(&format!(
                        "   • {} ({}) — {}\n",
                        member.email,
                        member.name.as_deref().unwrap_or("no name"),
                        member.role,
                    ));
                }
                output
            }
            None => "   No organization configured.".to_string(),
        }
    }

    /// Add a team member
    pub fn add_member(&mut self, email: &str, role: &str, name: Option<&str>) -> anyhow::Result<String> {
        let org = self.organization.as_mut().ok_or_else(|| anyhow::anyhow!("No organization configured"))?;

        let role = match role {
            "admin" => OrgRole::Admin,
            "member" => OrgRole::Member,
            "viewer" => OrgRole::Viewer,
            _ => anyhow::bail!("Invalid role: {role}. Use: admin, member, viewer"),
        };

        if org.members.iter().any(|m| m.email == email) {
            anyhow::bail!("Member '{email}' already exists");
        }

        org.members.push(TeamMember {
            email: email.to_string(),
            name: name.map(String::from),
            role: role.clone(),
            added_at: chrono::Utc::now().to_rfc3339(),
            last_active: None,
        });

        self.save()?;
        Ok(format!("   ✅ Added {email} as {role}"))
    }

    /// Remove a team member
    pub fn remove_member(&mut self, email: &str) -> anyhow::Result<String> {
        let org = self.organization.as_mut().ok_or_else(|| anyhow::anyhow!("No organization configured"))?;

        let pos = org.members.iter().position(|m| m.email == email)
            .ok_or_else(|| anyhow::anyhow!("Member '{email}' not found"))?;

        if org.members[pos].role == OrgRole::Admin && org.members.iter().filter(|m| m.role == OrgRole::Admin).count() <= 1 {
            anyhow::bail!("Cannot remove the last admin");
        }

        org.members.remove(pos);
        self.save()?;
        Ok(format!("   ✅ Removed {email}"))
    }

    /// Add OIDC provider
    pub fn add_oidc(&mut self, name: &str, issuer: &str, client_id: &str) -> anyhow::Result<String> {
        let org = self.organization.as_mut().ok_or_else(|| anyhow::anyhow!("No organization configured"))?;

        org.oidc_providers.push(OidcProvider {
            name: name.to_string(),
            issuer_url: issuer.to_string(),
            client_id: client_id.to_string(),
            client_secret: None,
            scopes: vec!["openid".into(), "profile".into(), "email".into()],
        });

        self.save()?;
        Ok(format!("   ✅ Added OIDC provider '{name}'"))
    }

    /// Check if a user has permission for an action
    pub fn check_permission(&self, email: &str, action: &str) -> bool {
        let org = match &self.organization {
            Some(o) => o,
            None => return true, // No org = no restrictions
        };

        let member = match org.members.iter().find(|m| m.email == email) {
            Some(m) => m,
            None => return false, // Not a member
        };

        match member.role {
            OrgRole::Admin => true, // Admin can do everything
            OrgRole::Member => {
                // Member cannot do restricted actions
                !org.policies.require_approval_for_commands.contains(&action.to_string())
            }
            OrgRole::Viewer => false, // Viewer can't do anything
        }
    }

    /// Add audit log entry
    pub fn audit_log(&self, entry: &str) {
        let org = match &self.organization {
            Some(o) => o,
            None => return,
        };
        if !org.policies.audit_logging {
            return;
        }
        // Write to audit log file
        if let Some(parent) = self.org_path.parent() {
            let audit_path = parent.join("audit.log");
            let log_entry = format!("[{}] {}\n", chrono::Utc::now().to_rfc3339(), entry);
            let _ = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&audit_path)
                .map(|mut f| {
                    use std::io::Write;
                    let _ = f.write_all(log_entry.as_bytes());
                });
        }
    }

    /// Check if an organization exists
    pub fn exists(&self) -> bool {
        self.organization.is_some()
    }
}

/// CLI subcommand helpers for org
pub fn print_org_help() {
    println!("Organization Commands:");
    println!("  hyper org init <name> --owner <email>   Create organization");
    println!("  hyper org status                        Show organization info");
    println!("  hyper org team add <email> [--role <r>]  Add team member");
    println!("  hyper org team remove <email>            Remove team member");
    println!("  hyper org team list                      List team members");
    println!("  hyper org oidc add <name> <issuer> <id>  Add OIDC provider");
    println!("  hyper org policy set <key> <value>       Set organization policy");
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn setup_test_org() -> (tempfile::TempDir, OrgManager) {
        let dir = tempfile::tempdir().unwrap();
        let mut mgr = OrgManager::new(dir.path());
        mgr.init("Test Corp", "admin@test.com").unwrap();
        (dir, mgr)
    }

    #[test]
    fn test_init() {
        let (_dir, mgr) = setup_test_org();
        assert!(mgr.exists());
        let info = mgr.info();
        assert!(info.contains("Test Corp"));
        assert!(info.contains("admin@test.com"));
    }

    #[test]
    fn test_add_member() {
        let (_dir, mut mgr) = setup_test_org();
        mgr.add_member("bob@test.com", "member", Some("Bob")).unwrap();
        let members = mgr.list_members();
        assert!(members.contains("bob@test.com"));
        assert!(members.contains("Bob"));
    }

    #[test]
    fn test_remove_member() {
        let (_dir, mut mgr) = setup_test_org();
        mgr.add_member("bob@test.com", "member", None).unwrap();
        mgr.remove_member("bob@test.com").unwrap();
        let members = mgr.list_members();
        assert!(!members.contains("bob@test.com"));
    }

    #[test]
    fn test_permissions() {
        let (_dir, mut mgr) = setup_test_org();
        mgr.add_member("dev@test.com", "member", None).unwrap();
        mgr.add_member("viewer@test.com", "viewer", None).unwrap();

        assert!(mgr.check_permission("admin@test.com", "deploy"));
        assert!(!mgr.check_permission("dev@test.com", "deploy")); // deploy is restricted
        assert!(mgr.check_permission("dev@test.com", "run"));
        assert!(!mgr.check_permission("viewer@test.com", "run"));
    }

    #[test]
    fn test_oidc() {
        let (_dir, mut mgr) = setup_test_org();
        mgr.add_oidc("google", "https://accounts.google.com", "client-123").unwrap();
        let info = mgr.info();
        assert!(info.contains("1 provider"));
    }

    #[test]
    fn test_audit_log() {
        let (_dir, mgr) = setup_test_org();
        mgr.audit_log("Test audit entry");
        // Should not crash
    }
}
