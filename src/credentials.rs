use serde::Deserialize;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

const CREATE_NO_WINDOW: u32 = 0x08000000;

#[derive(Debug, Deserialize)]
struct ClaudeCredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<ClaudeOAuthEntry>,
}

#[derive(Debug, Deserialize)]
struct ClaudeOAuthEntry {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "expiresAt")]
    expires_at: u64,
}

impl ClaudeOAuthEntry {
    fn remaining_ms(&self) -> i64 {
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        self.expires_at as i64 - now_ms as i64
    }
}

#[derive(Debug, Deserialize)]
struct CodexCredentialsFile {
    tokens: Option<CodexTokenEntry>,
}

#[derive(Debug, Deserialize)]
struct CodexTokenEntry {
    access_token: String,
}

#[derive(Debug, Clone)]
pub enum ClaudeAuth {
    BearerToken(String),
    Unsupported(String),
}

pub fn read_claude_auth() -> Option<ClaudeAuth> {
    if uses_claude_cloud_provider() {
        return Some(ClaudeAuth::Unsupported(
            "cloud provider authentication is selected by Claude Code".to_string(),
        ));
    }

    if let Some(token) = non_empty_env("ANTHROPIC_AUTH_TOKEN") {
        return Some(ClaudeAuth::BearerToken(strip_bearer_prefix(&token)));
    }

    if non_empty_env("ANTHROPIC_API_KEY").is_some() {
        return Some(ClaudeAuth::Unsupported(
            "ANTHROPIC_API_KEY does not expose Claude Code 5H/7D OAuth usage headers".to_string(),
        ));
    }

    if let Some(token) = non_empty_env("CLAUDE_CODE_OAUTH_TOKEN") {
        return Some(ClaudeAuth::BearerToken(strip_bearer_prefix(&token)));
    }

    let mut candidates = Vec::new();

    if let Some(p) = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .map(|p| p.join(".credentials.json"))
    {
        if p.exists() {
            candidates.push(p);
        }
    }
    if let Some(p) = dirs::home_dir().map(|h| h.join(".claude").join(".credentials.json")) {
        if p.exists() {
            candidates.push(p);
        }
    }
    candidates.extend(wsl_credential_paths(".claude", ".credentials.json"));

    if let Some(token) = read_best_claude_token_from(&candidates) {
        return Some(ClaudeAuth::BearerToken(token));
    }

    if refresh_claude_token_via_cli() {
        return read_best_claude_token_from(&candidates).map(ClaudeAuth::BearerToken);
    }

    None
}

fn uses_claude_cloud_provider() -> bool {
    [
        "CLAUDE_CODE_USE_BEDROCK",
        "CLAUDE_CODE_USE_VERTEX",
        "CLAUDE_CODE_USE_FOUNDRY",
    ]
    .iter()
    .any(|name| non_empty_env(name).is_some())
}

fn non_empty_env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn strip_bearer_prefix(token: &str) -> String {
    token
        .strip_prefix("Bearer ")
        .or_else(|| token.strip_prefix("bearer "))
        .unwrap_or(token)
        .trim()
        .to_string()
}

fn read_best_claude_token_from(candidates: &[PathBuf]) -> Option<String> {
    let mut best_token: Option<(String, i64)> = None;

    for path in candidates {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(creds) = serde_json::from_str::<ClaudeCredentialsFile>(&data) {
                if let Some(oauth) = creds.claude_ai_oauth {
                    let remaining = oauth.remaining_ms();
                    if remaining > 0 {
                        match &best_token {
                            Some((_, best_remaining)) if remaining <= *best_remaining => {}
                            _ => best_token = Some((oauth.access_token, remaining)),
                        }
                    }
                }
            }
        }
    }

    best_token.map(|(token, _)| token)
}

pub fn read_codex_token() -> Option<String> {
    let mut candidates = Vec::new();

    if let Some(p) = dirs::home_dir().map(|h| h.join(".codex").join("auth.json")) {
        if p.exists() {
            candidates.push(p);
        }
    }
    candidates.extend(wsl_credential_paths(".codex", "auth.json"));

    for path in candidates {
        if let Ok(data) = std::fs::read_to_string(&path) {
            if let Ok(creds) = serde_json::from_str::<CodexCredentialsFile>(&data) {
                if let Some(tokens) = creds.tokens {
                    if !tokens.access_token.trim().is_empty() {
                        return Some(tokens.access_token);
                    }
                }
            }
        }
    }

    None
}

fn wsl_credential_paths(config_dir: &str, file_name: &str) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for base in [r"\\wsl$", r"\\wsl.localhost"] {
        if let Ok(entries) = std::fs::read_dir(PathBuf::from(base)) {
            for entry in entries.flatten() {
                let home = entry.path().join("home");
                if let Ok(users) = std::fs::read_dir(&home) {
                    for user in users.flatten() {
                        let cred = user.path().join(config_dir).join(file_name);
                        if cred.exists() {
                            paths.push(cred);
                        }
                    }
                }
            }
        }
    }
    paths
}

fn refresh_claude_token_via_cli() -> bool {
    std::process::Command::new("claude")
        .args(["-p", "say hi", "--max-turns", "1"])
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false)
}
