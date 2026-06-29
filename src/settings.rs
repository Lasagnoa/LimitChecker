use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// デフォルト値定数
const DEFAULT_POLL_MS: u32 = 60_000; // デフォルト更新間隔: 1分
const DEFAULT_LOG_ENABLED: bool = false; // デバッグログ: OFF
const DEFAULT_STATUS_JSON_ENABLED: bool = true; // status.json 書き出し: ON

fn default_status_json_enabled() -> bool {
    DEFAULT_STATUS_JSON_ENABLED
}

/// アプリケーション設定
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    /// APIポーリング間隔（ミリ秒）
    pub poll_interval_ms: u32,
    /// デバッグログ出力ON/OFF
    #[serde(default)]
    pub log_enabled: bool,
    /// status.json 書き出しON/OFF
    #[serde(default = "default_status_json_enabled")]
    pub status_json_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            poll_interval_ms: DEFAULT_POLL_MS,
            log_enabled: DEFAULT_LOG_ENABLED,
            status_json_enabled: DEFAULT_STATUS_JSON_ENABLED,
        }
    }
}

/// 設定ファイルのパスを返す (%APPDATA%\LimitChecker\settings.json)
fn settings_path() -> Option<PathBuf> {
    std::env::var("APPDATA").ok().map(|appdata| {
        PathBuf::from(appdata)
            .join("LimitChecker")
            .join("settings.json")
    })
}

/// 設定をファイルからロードする。失敗時はデフォルト設定を返す
pub fn load() -> Settings {
    let Some(path) = settings_path() else {
        return Settings::default();
    };
    let Ok(data) = std::fs::read_to_string(&path) else {
        return Settings::default();
    };
    serde_json::from_str::<Settings>(&data).unwrap_or_default()
}

/// 設定をファイルに保存する
pub fn save(settings: &Settings) {
    let Some(path) = settings_path() else { return };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(data) = serde_json::to_string_pretty(settings) {
        let _ = std::fs::write(&path, data);
    }
}
