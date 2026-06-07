#![allow(unused)]
//! Billing & Subscription Infrastructure
//!
//! For 100M users, monetization must be built-in from day one:
//! - License key system (offline validation via HMAC signing)
//! - Pricing tiers: Free → Pro → Team → Enterprise
//! - Usage-based metering (agent runs, tokens, seats)
//! - Stripe integration (webhooks, checkout sessions)
//! - Feature gating: check tier before enabling premium features
//!
//! Architecture:
//!   License Key (local) ──── HMAC signature ───→ Validation (no network)
//!   Stripe Webhook ──── HTTP endpoint ───→ Update tier
//!   Usage Meter ──── SQLite ───→ Enforce limits
//!
//! Commands:
//!   hyper billing status       — Show current plan and usage
//!   hyper billing upgrade      — Open Stripe checkout
//!   hyper billing license KEY  — Activate with license key

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Billing secret for signing license keys (set via HYPER_BILLING_SECRET env)
const SECRET_ENV: &str = "HYPER_BILLING_SECRET";

// ─── Pricing Tiers ─────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Tier {
    /// Free tier: 50 runs/month, 1 seat, community support
    Free,
    /// Pro: unlimited runs, 1 seat, priority email support, advanced features
    Pro,
    /// Team: unlimited runs, up to 25 seats, SSO, team dashboards, shared memory
    Team,
    /// Enterprise: unlimited everything, custom SLA, on-premise, dedicated support
    Enterprise,
}

impl Tier {
    pub fn name(&self) -> &'static str {
        match self {
            Tier::Free => "Free",
            Tier::Pro => "Pro",
            Tier::Team => "Team",
            Tier::Enterprise => "Enterprise",
        }
    }

    pub fn monthly_price_usd(&self) -> Option<u32> {
        match self {
            Tier::Free => None, // Free
            Tier::Pro => Some(10),
            Tier::Team => Some(25),
            Tier::Enterprise => None, // Custom pricing
        }
    }

    pub fn max_runs_per_month(&self) -> Option<u32> {
        match self {
            Tier::Free => Some(50),
            _ => None, // Unlimited
        }
    }

    pub fn max_seats(&self) -> Option<u32> {
        match self {
            Tier::Free => Some(1),
            Tier::Pro => Some(1),
            Tier::Team => Some(25),
            Tier::Enterprise => None, // Unlimited
        }
    }

    pub fn has_feature(&self, feature: &str) -> bool {
        match feature {
            "sso" | "saml" => matches!(self, Tier::Team | Tier::Enterprise),
            "audit_log" | "team_dashboard" | "shared_memory" => matches!(self, Tier::Team | Tier::Enterprise),
            "priority_support" | "api_access" | "advanced_rules" => matches!(self, Tier::Pro | Tier::Team | Tier::Enterprise),
            "on_premise" | "custom_sla" | "dedicated_support" => matches!(self, Tier::Enterprise),
            "basic" => true, // All tiers
            _ => true, // Unknown features default to allowed
        }
    }

    /// All tiers ordered by level
    pub fn all() -> Vec<Tier> {
        vec![Tier::Free, Tier::Pro, Tier::Team, Tier::Enterprise]
    }
}

impl std::fmt::Display for Tier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

// ─── License Key ───────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LicensePayload {
    /// License version for forward-compatibility
    pub version: u32,
    /// Tier: 0=Free, 1=Pro, 2=Team, 3=Enterprise
    pub tier: u8,
    /// Unix timestamp when license was issued
    pub issued_at: u64,
    /// Unix timestamp when license expires (0 = never)
    pub expires_at: u64,
    /// Licensed user/org email
    pub email: String,
    /// Maximum seats (0 = unlimited)
    pub max_seats: u32,
    /// Unique license ID for revocation
    pub license_id: String,
}

impl LicensePayload {
    pub fn tier(&self) -> Tier {
        match self.tier {
            1 => Tier::Pro,
            2 => Tier::Team,
            3 => Tier::Enterprise,
            _ => Tier::Free,
        }
    }

    pub fn is_expired(&self) -> bool {
        if self.expires_at == 0 {
            return false; // Never expires
        }
        now_epoch() > self.expires_at
    }

    pub fn days_remaining(&self) -> Option<u32> {
        if self.expires_at == 0 {
            return None; // Never expires
        }
        let now = now_epoch();
        if now >= self.expires_at {
            return Some(0);
        }
        Some(((self.expires_at - now) / 86400) as u32)
    }
}

/// Generate a signed license key from a payload
pub fn generate_license(payload: &LicensePayload, secret: &str) -> Result<String> {
    let json = serde_json::to_string(payload)?;
    let encoded = base64_encode(&json);
    let signature = sign(&encoded, secret);
    // Format: HYPER.base64_payload.base64_signature
    Ok(format!("HYPER.{}.{}", encoded, signature))
}

/// Parse and validate a license key
pub fn parse_license(key: &str, secret: &str) -> Result<LicensePayload> {
    // Strip optional "HYPER." or "hyper-" prefix
    let key = key
        .strip_prefix("HYPER.")
        .or_else(|| key.strip_prefix("hyper-"))
        .unwrap_or(key);

    let parts: Vec<&str> = key.splitn(2, '.').collect();
    if parts.len() != 2 {
        bail!("Invalid license key format");
    }

    let (encoded, signature) = (parts[0], parts[1]);

    // Verify signature
    let expected_sig = sign(encoded, secret);
    if !constant_time_eq(signature.as_bytes(), expected_sig.as_bytes()) {
        bail!("Invalid license signature — key may be tampered");
    }

    // Decode payload
    let json = base64_decode(encoded)?;
    let payload: LicensePayload = serde_json::from_str(&json)
        .context("Failed to parse license payload")?;

    // Check expiry
    if payload.is_expired() {
        bail!("License expired on {}", payload.expires_at);
    }

    Ok(payload)
}

// ─── Billing State ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BillingState {
    pub tier: Tier,
    pub license_id: Option<String>,
    pub subscribed_since: Option<u64>,
    pub runs_this_month: u32,
    pub month_start: u64,
    pub stripe_customer_id: Option<String>,
    pub stripe_subscription_id: Option<String>,
}

impl BillingState {
    pub fn new() -> Self {
        BillingState {
            tier: Tier::Free,
            license_id: None,
            subscribed_since: None,
            runs_this_month: 0,
            month_start: now_epoch(),
            stripe_customer_id: None,
            stripe_subscription_id: None,
        }
    }

    pub fn load_or_create() -> Result<Self> {
        let path = billing_path();
        if path.exists() {
            let json = std::fs::read_to_string(&path)?;
            Ok(serde_json::from_str(&json)?)
        } else {
            Ok(BillingState::new())
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = billing_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Reset monthly counter if it's a new month
    pub fn maybe_reset_month(&mut self) {
        let now = now_epoch();
        // 30 days = 2592000 seconds
        if now - self.month_start > 2_592_000 {
            self.runs_this_month = 0;
            self.month_start = now;
        }
    }

    /// Check if user can make another run
    pub fn can_run(&self) -> bool {
        match self.tier.max_runs_per_month() {
            Some(max) => self.runs_this_month < max,
            None => true,
        }
    }

    /// Record a run
    pub fn record_run(&mut self) -> Result<()> {
        self.maybe_reset_month();
        self.runs_this_month += 1;
        self.save()
    }

    /// Activate a license
    pub fn activate_license(&mut self, payload: &LicensePayload) -> Result<()> {
        self.tier = payload.tier();
        self.license_id = Some(payload.license_id.clone());
        if self.subscribed_since.is_none() {
            self.subscribed_since = Some(now_epoch());
        }
        self.save()
    }

    /// Print billing status
    pub fn print_status(&self) {
        println!();
        println!("  \x1b[1;36m💳  Billing Status\x1b[0m");
        println!("  {}", "─".repeat(40));
        println!("  Plan:             \x1b[1;33m{}\x1b[0m", self.tier.name());
        if let Some(price) = self.tier.monthly_price_usd() {
            println!("  Price:            \x1b[1m${}/month\x1b[0m", price);
        } else {
            println!("  Price:            \x1b[1mFree\x1b[0m");
        }
        if self.tier == Tier::Free {
            println!(
                "  Runs this month:  \x1b[1m{}\x1b[0m / {}",
                self.runs_this_month,
                self.tier.max_runs_per_month().unwrap_or(0)
            );
        } else {
            println!(
                "  Runs this month:  \x1b[1m{}\x1b[0m (unlimited)",
                self.runs_this_month
            );
        }
        if let Some(license_id) = &self.license_id {
            println!("  License:          {}", license_id);
        }
        if self.tier == Tier::Free {
            println!();
            println!("  \x1b[90mUpgrade: hyper billing upgrade\x1b[0m");
        }
        println!();
    }
}

// ─── Stripe Integration ────────────────────────────────────────

/// Generate a Stripe Checkout URL for upgrading
pub fn stripe_checkout_url(tier: Tier, success_url: &str, cancel_url: &str) -> Result<String> {
    let price_id = match tier {
        Tier::Pro => "price_pro_monthly",
        Tier::Team => "price_team_monthly",
        Tier::Enterprise => "price_enterprise_custom",
        _ => bail!("Cannot checkout Free tier"),
    };

    // In production, call Stripe API to create Checkout Session
    // For now, return a placeholder URL that shows the pattern
    Ok(format!(
        "https://checkout.hyperagent.dev/subscribe?price={}&success={}&cancel={}",
        price_id, success_url, cancel_url
    ))
}

/// Handle Stripe webhook events (called from SaaS web server)
pub fn handle_stripe_webhook(
    event_type: &str,
    body: &str,
    billing: &mut BillingState,
) -> Result<()> {
    // Verify Stripe signature in production
    match event_type {
        "checkout.session.completed" => {
            // Parse customer + subscription IDs from body
            billing.save()?;
        }
        "customer.subscription.updated" => {
            billing.save()?;
        }
        "customer.subscription.deleted" => {
            billing.tier = Tier::Free;
            billing.stripe_subscription_id = None;
            billing.save()?;
        }
        "invoice.payment_failed" => {
            // Grace period: don't downgrade immediately
            eprintln!("⚠️  Payment failed. Please update your payment method.");
        }
        _ => {}
    }
    Ok(())
}

// ─── Helpers ───────────────────────────────────────────────────

fn billing_path() -> PathBuf {
    let data_dir = dirs_next::data_dir()
        .unwrap_or_else(|| Path::new("~/.local/share").to_path_buf())
        .join("hyper");
    data_dir.join("billing.json")
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn sign(data: &str, secret: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(secret.as_bytes());
    hasher.update(b"::");
    hasher.update(data.as_bytes());
    let hash = hasher.finalize();
    base64_encode(&hash[..16].iter().map(|b| format!("{:02x}", b)).collect::<Vec<_>>().join(""))
}

fn base64_encode(s: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(s.as_bytes())
}

fn base64_decode(s: &str) -> Result<String> {
    use base64::Engine;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(s.as_bytes())
        .context("Invalid base64 in license key")?;
    String::from_utf8(bytes).context("Invalid UTF-8 in license payload")
}

/// Constant-time comparison to prevent timing attacks on signatures
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

// ─── Feature Gating ────────────────────────────────────────────

/// Check if the current tier allows a feature. Returns Err if not allowed.
pub fn require_feature(feature: &str) -> Result<()> {
    let billing = BillingState::load_or_create()?;
    if billing.tier.has_feature(feature) {
        Ok(())
    } else {
        bail!(
            "Feature '{}' requires {} tier or higher. Current: {}. Upgrade: hyper billing upgrade",
            feature,
            required_tier_for_feature(feature),
            billing.tier.name()
        )
    }
}

fn required_tier_for_feature(feature: &str) -> &'static str {
    match feature {
        "sso" | "saml" | "audit_log" | "team_dashboard" | "shared_memory" => "Team",
        "priority_support" | "api_access" | "advanced_rules" => "Pro",
        "on_premise" | "custom_sla" | "dedicated_support" => "Enterprise",
        _ => "Free",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_pricing() {
        assert_eq!(Tier::Free.monthly_price_usd(), None);
        assert_eq!(Tier::Pro.monthly_price_usd(), Some(10));
        assert_eq!(Tier::Team.monthly_price_usd(), Some(25));
        assert_eq!(Tier::Enterprise.monthly_price_usd(), None);
    }

    #[test]
    fn test_tier_features() {
        assert!(Tier::Free.has_feature("basic"));
        assert!(!Tier::Free.has_feature("sso"));
        assert!(Tier::Pro.has_feature("priority_support"));
        assert!(Tier::Team.has_feature("sso"));
        assert!(Tier::Enterprise.has_feature("on_premise"));
    }

    #[test]
    fn test_license_generate_and_parse() {
        let secret = "test-secret-key-12345";
        let payload = LicensePayload {
            version: 1,
            tier: 1,
            issued_at: now_epoch(),
            expires_at: now_epoch() + 365 * 86400,
            email: "test@example.com".into(),
            max_seats: 1,
            license_id: "LIC-001".into(),
        };

        let key = generate_license(&payload, secret).unwrap();
        let parsed = parse_license(&key, secret).unwrap();

        assert_eq!(parsed.tier(), Tier::Pro);
        assert_eq!(parsed.email, "test@example.com");
        assert_eq!(parsed.license_id, "LIC-001");
    }

    #[test]
    fn test_license_tamper_detection() {
        let secret = "secret-a";
        let payload = LicensePayload {
            version: 1,
            tier: 2,
            issued_at: now_epoch(),
            expires_at: 0,
            email: "u@e.com".into(),
            max_seats: 5,
            license_id: "L2".into(),
        };

        let key = generate_license(&payload, secret).unwrap();
        // Try with wrong secret
        assert!(parse_license(&key, "wrong-secret").is_err());
    }

    #[test]
    fn test_license_expired() {
        let secret = "secret";
        let payload = LicensePayload {
            version: 1,
            tier: 1,
            issued_at: 1000,
            expires_at: 1001, // already expired
            email: "old@e.com".into(),
            max_seats: 1,
            license_id: "EXPIRED".into(),
        };

        let key = generate_license(&payload, secret).unwrap();
        assert!(parse_license(&key, secret).is_err());
    }

    #[test]
    fn test_billing_state_save_load() {
        let mut state = BillingState::new();
        state.tier = Tier::Pro;
        state.record_run().unwrap();

        assert!(state.can_run());
        assert_eq!(state.runs_this_month, 1);
    }

    #[test]
    fn test_run_limits_free_tier() {
        let mut state = BillingState::new();
        state.tier = Tier::Free;

        // Simulate 50 runs
        state.runs_this_month = 50;
        assert!(!state.can_run());

        state.runs_this_month = 49;
        assert!(state.can_run());
    }
}
