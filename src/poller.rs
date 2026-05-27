use crate::credentials;
use chrono::{DateTime, Utc};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct ProviderUsage {
    pub session_pct: f64,
    pub session_resets_at: Option<DateTime<Utc>>,
    pub weekly_pct: f64,
    pub weekly_resets_at: Option<DateTime<Utc>>,
}

impl Default for ProviderUsage {
    fn default() -> Self {
        Self {
            session_pct: 0.0,
            session_resets_at: None,
            weekly_pct: 0.0,
            weekly_resets_at: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UsageInfo {
    pub claude: ProviderUsage,
    /// Claude のトークンが見つかり使用率取得が可能かどうか
    pub claude_configured: bool,
    pub codex: ProviderUsage,
    /// Codex のトークンが見つかり使用率取得が可能かどうか
    pub codex_configured: bool,
}

impl Default for UsageInfo {
    fn default() -> Self {
        Self {
            claude: ProviderUsage::default(),
            // 初回ポーリング完了までは両方表示を維持する
            claude_configured: true,
            codex: ProviderUsage::default(),
            codex_configured: true,
        }
    }
}

pub type SharedUsageInfo = Arc<Mutex<UsageInfo>>;

pub fn create_shared_info() -> SharedUsageInfo {
    Arc::new(Mutex::new(UsageInfo::default()))
}

fn write_log(message: &str) {
    if !crate::LOG_ENABLED.load(std::sync::atomic::Ordering::Relaxed) {
        return;
    }

    let log_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("limitchecker.log")));

    if let Some(path) = log_path {
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
        {
            let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
            let _ = writeln!(f, "[{}] {}", now, message);
        }
    }
}

fn build_client() -> reqwest::blocking::Client {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .unwrap()
}

pub fn poll_once(shared: &SharedUsageInfo) {
    let client = build_client();
    let previous = shared.lock().unwrap().clone();

    // Claude: トークン種別を先に確認してから取得
    let claude_auth = credentials::read_claude_auth();
    let claude_configured = matches!(claude_auth, Some(credentials::ClaudeAuth::BearerToken(_)));
    let claude = match claude_auth {
        Some(credentials::ClaudeAuth::BearerToken(token)) => fetch_claude_usage(&client, &token)
            .unwrap_or_else(|e| {
                write_log(&format!("Claude usage failed: {}", e));
                previous.claude.clone()
            }),
        Some(credentials::ClaudeAuth::Unsupported(reason)) => {
            write_log(&format!("Claude usage skipped: {}", reason));
            previous.claude.clone()
        }
        None => {
            write_log("Claude usage skipped: no valid token found");
            previous.claude.clone()
        }
    };

    // Codex: トークンの有無を先に確認してから取得
    let codex_token = credentials::read_codex_token();
    let codex_configured = codex_token.is_some();
    let codex = match codex_token {
        Some(token) => fetch_codex_usage(&client, &token).unwrap_or_else(|e| {
            write_log(&format!("Codex usage failed: {}", e));
            previous.codex.clone()
        }),
        None => {
            write_log("Codex usage skipped: no valid token found");
            previous.codex.clone()
        }
    };

    let mut info = shared.lock().unwrap();
    info.claude = claude;
    info.claude_configured = claude_configured;
    info.codex = codex;
    info.codex_configured = codex_configured;
}

fn fetch_claude_usage(
    client: &reqwest::blocking::Client,
    token: &str,
) -> Result<ProviderUsage, String> {
    write_log("Claude usage fetch started");
    let body = serde_json::json!({
        "model": "claude-haiku-4-5-20251001",
        "max_tokens": 1,
        "messages": [{"role": "user", "content": "."}]
    });

    let resp = client
        .post("https://api.anthropic.com/v1/messages")
        .header("Authorization", format!("Bearer {}", token))
        .header("anthropic-version", "2023-06-01")
        .header("anthropic-beta", "oauth-2025-04-20")
        .header("content-type", "application/json")
        .body(body.to_string())
        .send()
        .map_err(|e| format!("Messages API request failed: {}", e))?;

    let status = resp.status();
    let headers = resp.headers().clone();
    write_log(&format!("Claude Messages API status: {}", status));

    if !status.is_success() && status.as_u16() != 429 {
        let body_text = resp.text().unwrap_or_default();
        write_log(&format!(
            "Claude Messages API HTTP {}: {}",
            status, body_text
        ));
        return Err(format!("Claude Messages API HTTP {}", status));
    }

    let session_pct = unified_utilization_pct(
        &headers,
        "anthropic-ratelimit-unified-5h-utilization",
        "anthropic-ratelimit-unified-5h-status",
    )
    .unwrap_or(if status.as_u16() == 429 { 100.0 } else { 0.0 });
    let weekly_pct = unified_utilization_pct(
        &headers,
        "anthropic-ratelimit-unified-7d-utilization",
        "anthropic-ratelimit-unified-7d-status",
    )
    .unwrap_or(if status.as_u16() == 429 { 100.0 } else { 0.0 });
    let session_reset = timestamp_header(&headers, "anthropic-ratelimit-unified-5h-reset")
        .or_else(|| timestamp_header(&headers, "anthropic-ratelimit-unified-reset"));
    let weekly_reset = timestamp_header(&headers, "anthropic-ratelimit-unified-7d-reset")
        .or_else(|| timestamp_header(&headers, "anthropic-ratelimit-unified-reset"));

    write_log(&format!(
        "Claude usage fetched: 5h={:.1}%, weekly={:.1}%",
        session_pct, weekly_pct
    ));

    Ok(ProviderUsage {
        session_pct,
        session_resets_at: session_reset,
        weekly_pct,
        weekly_resets_at: weekly_reset,
    })
}

fn fetch_codex_usage(
    client: &reqwest::blocking::Client,
    token: &str,
) -> Result<ProviderUsage, String> {
    write_log("Codex usage fetch started");
    let resp = client
        .get("https://chatgpt.com/backend-api/wham/usage")
        .header("Authorization", format!("Bearer {}", token))
        .header("accept", "application/json")
        .header("user-agent", "LimitChecker/0.1.0")
        .send()
        .map_err(|e| format!("Codex usage API request failed: {}", e))?;

    let status = resp.status();
    write_log(&format!("Codex usage API status: {}", status));

    if !status.is_success() {
        let body_text = resp.text().unwrap_or_default();
        write_log(&format!("Codex usage API HTTP {}: {}", status, body_text));
        return Err(format!("Codex usage API HTTP {}", status));
    }

    let usage = resp
        .json::<serde_json::Value>()
        .map_err(|e| format!("Codex usage API JSON parse failed: {}", e))?;

    let primary = usage.pointer("/rate_limit/primary_window");
    let secondary = usage.pointer("/rate_limit/secondary_window");
    let session_pct = primary
        .and_then(|v| v.get("used_percent"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let weekly_pct = secondary
        .and_then(|v| v.get("used_percent"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);

    let account_name = usage
        .get("email")
        .or_else(|| usage.get("user_id"))
        .and_then(|v| v.as_str())
        .unwrap_or("Codex")
        .to_string();
    let plan = usage
        .get("plan_type")
        .and_then(|v| v.as_str())
        .unwrap_or("Codex")
        .to_string();

    write_log(&format!(
        "Codex usage fetched: 5h={:.1}%, weekly={:.1}%, account={}, plan={}",
        session_pct, weekly_pct, account_name, plan
    ));

    Ok(ProviderUsage {
        session_pct,
        session_resets_at: parse_codex_reset(primary),
        weekly_pct,
        weekly_resets_at: parse_codex_reset(secondary),
    })
}

fn unified_utilization_pct(
    headers: &reqwest::header::HeaderMap,
    utilization_key: &str,
    status_key: &str,
) -> Option<f64> {
    if let Some(v) = headers
        .get(utilization_key)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_unified_utilization_pct)
    {
        return Some(v);
    }

    if is_exhausted_status(headers, status_key)
        || is_exhausted_status(headers, "anthropic-ratelimit-unified-status")
    {
        return Some(100.0);
    }

    None
}

fn parse_unified_utilization_pct(raw: &str) -> Option<f64> {
    let ratio = raw.trim().parse::<f64>().ok()?;
    if ratio.is_finite() {
        Some((ratio * 100.0).max(0.0))
    } else {
        None
    }
}

fn is_exhausted_status(headers: &reqwest::header::HeaderMap, key: &str) -> bool {
    headers
        .get(key)
        .and_then(|v| v.to_str().ok())
        .map(|v| matches!(v.trim(), "rate_limited" | "exceeded" | "rejected"))
        .unwrap_or(false)
}

fn timestamp_header(headers: &reqwest::header::HeaderMap, key: &str) -> Option<DateTime<Utc>> {
    headers
        .get(key)
        .and_then(|v| v.to_str().ok())
        .and_then(parse_timestamp)
}

fn parse_timestamp(raw: &str) -> Option<DateTime<Utc>> {
    let value = raw.trim();
    if let Ok(ts) = value.parse::<i64>() {
        return DateTime::from_timestamp(ts, 0).map(|dt| dt.with_timezone(&Utc));
    }

    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn parse_codex_reset(window: Option<&serde_json::Value>) -> Option<DateTime<Utc>> {
    if let Some(ts) = window
        .and_then(|v| v.get("reset_at"))
        .and_then(|v| v.as_i64())
    {
        return DateTime::from_timestamp(ts, 0).map(|dt| dt.with_timezone(&Utc));
    }

    window
        .and_then(|v| v.get("reset_after_seconds"))
        .and_then(|v| v.as_i64())
        .map(|seconds| Utc::now() + chrono::Duration::seconds(seconds))
}

pub fn format_reset_local(reset: &DateTime<Utc>) -> String {
    use chrono::Local;
    let local = reset.with_timezone(&Local);
    let offset = local.offset();
    let total_secs = offset.local_minus_utc();
    let hours = total_secs / 3600;
    let mins = (total_secs.abs() % 3600) / 60;
    let utc_str = if mins == 0 {
        format!("UTC{:+}", hours)
    } else {
        format!("UTC{:+}:{:02}", hours, mins)
    };
    format!("{} ({})", local.format("%m/%d %H:%M"), utc_str)
}

pub fn format_reset_countdown(reset: &DateTime<Utc>) -> String {
    let now = Utc::now();
    let diff = *reset - now;

    if diff.num_seconds() <= 0 {
        return "now".to_string();
    }

    let days = diff.num_days();
    let hours = diff.num_hours() % 24;
    let mins = diff.num_minutes() % 60;

    if days > 0 {
        format!("{}d {}h {}m", days, hours, mins)
    } else if hours > 0 {
        format!("{}h {}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    #[test]
    fn parses_unified_utilization_as_ratio_even_above_one() {
        assert_eq!(parse_unified_utilization_pct("0.42"), Some(42.0));
        assert_eq!(parse_unified_utilization_pct("1.02"), Some(102.0));
    }

    #[test]
    fn exhausted_status_falls_back_to_full_usage() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "anthropic-ratelimit-unified-5h-status",
            HeaderValue::from_static("rate_limited"),
        );

        assert_eq!(
            unified_utilization_pct(
                &headers,
                "anthropic-ratelimit-unified-5h-utilization",
                "anthropic-ratelimit-unified-5h-status",
            ),
            Some(100.0)
        );
    }

    #[test]
    fn missing_unified_headers_on_429_are_treated_as_full_usage() {
        let headers = HeaderMap::new();
        let pct = unified_utilization_pct(
            &headers,
            "anthropic-ratelimit-unified-5h-utilization",
            "anthropic-ratelimit-unified-5h-status",
        )
        .unwrap_or(100.0);

        assert_eq!(pct, 100.0);
    }

    #[test]
    fn parses_epoch_and_rfc3339_timestamps() {
        assert!(parse_timestamp("1765944000").is_some());
        assert!(parse_timestamp("2026-04-28T12:34:56Z").is_some());
    }
}
