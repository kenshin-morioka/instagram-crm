use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

/// アプリ全体の設定 (設定ファイル `config.json` から読み込む)
///
/// 定型返信文とポーリング間隔はUIから変更できるようSQLiteのsettingsに保存し、
/// ここでの値は初期値として扱う。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    /// 認証方式。現在は instagram_login のみ対応
    pub meta_auth_mode: String,
    /// Graph APIバージョン (Meta App Dashboardでサポート中のものを指定)
    pub meta_graph_api_version: String,
    /// ポーリング間隔の初期値 (秒)
    pub polling_interval_secs: u64,
    /// 何時間前までのコメントを返信対象とするか
    pub comment_lookback_hours: i64,
    /// 1回のポーリングで取得する自分のメディア件数
    pub media_fetch_limit: u32,
    /// trueの間は返信を送信せず、対象をログに出すだけ (安全のため既定ON)
    pub dry_run: bool,
    /// 返信対象とするリールのmedia_id。空なら自分の全リールが対象
    pub allowed_media_ids: Vec<String>,
    /// コメント本文に含まれる場合のみ返信するキーワード (大文字小文字無視)。空なら全コメント対象
    pub reply_keywords: Vec<String>,
    /// これより長いコメントは対象外 (文字数)
    pub max_comment_length: usize,
    /// Meta API使用量 (x-app-usage) がこの割合を超えたら送信を一時停止する
    pub usage_pause_threshold_pct: f64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            meta_auth_mode: "instagram_login".into(),
            meta_graph_api_version: "v25.0".into(),
            polling_interval_secs: 30,
            comment_lookback_hours: 24,
            media_fetch_limit: 10,
            dry_run: true,
            allowed_media_ids: Vec::new(),
            reply_keywords: Vec::new(),
            max_comment_length: 500,
            usage_pause_threshold_pct: 90.0,
        }
    }
}

impl AppConfig {
    pub fn validate(&self) -> AppResult<()> {
        if self.meta_auth_mode != "instagram_login" {
            return Err(AppError::Config(format!(
                "meta_auth_mode '{}' は未対応です (対応: instagram_login)",
                self.meta_auth_mode
            )));
        }
        if !self
            .meta_graph_api_version
            .strip_prefix('v')
            .is_some_and(|v| v.chars().all(|c| c.is_ascii_digit() || c == '.'))
        {
            return Err(AppError::Config(format!(
                "meta_graph_api_version '{}' が不正です (例: v25.0)",
                self.meta_graph_api_version
            )));
        }
        Ok(())
    }

    /// media_idが返信対象か (allowlistが空なら全リール対象)
    pub fn is_media_allowed(&self, media_id: &str) -> bool {
        self.allowed_media_ids.is_empty()
            || self.allowed_media_ids.iter().any(|id| id == media_id)
    }
}

/// 設定ファイルを読み込む。存在しなければ初期値で作成する
pub fn load_or_create(config_dir: &Path) -> AppResult<AppConfig> {
    let path = config_dir.join("config.json");
    let config: AppConfig = if path.exists() {
        let raw = fs::read_to_string(&path)?;
        serde_json::from_str(&raw)
            .map_err(|e| AppError::Config(format!("{} のパースに失敗: {}", path.display(), e)))?
    } else {
        fs::create_dir_all(config_dir)?;
        let config = AppConfig::default();
        let raw = serde_json::to_string_pretty(&config)
            .map_err(|e| AppError::Config(format!("設定のシリアライズに失敗: {}", e)))?;
        fs::write(&path, raw)?;
        log::info!("設定ファイルを作成しました: {}", path.display());
        config
    };
    config.validate()?;
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir()
            .join("instagram-crm-test")
            .join(format!("{}-{}", name, uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn creates_default_config_when_missing() {
        let dir = temp_dir("create");
        let config = load_or_create(&dir).unwrap();
        assert_eq!(config.polling_interval_secs, 30);
        assert!(config.dry_run, "安全のためドライランが既定でONであること");
        assert_eq!(config.meta_graph_api_version, "v25.0");
        assert!(dir.join("config.json").exists());
    }

    #[test]
    fn loads_existing_config() {
        let dir = temp_dir("load");
        fs::write(
            dir.join("config.json"),
            r#"{"polling_interval_secs":60,"dry_run":false,"meta_graph_api_version":"v26.0"}"#,
        )
        .unwrap();
        let config = load_or_create(&dir).unwrap();
        assert_eq!(config.polling_interval_secs, 60);
        assert!(!config.dry_run);
        assert_eq!(config.meta_graph_api_version, "v26.0");
        // 省略されたキーは初期値で補完される
        assert_eq!(config.comment_lookback_hours, 24);
        assert_eq!(config.max_comment_length, 500);
    }

    #[test]
    fn ignores_unknown_keys_for_backward_compatibility() {
        // 旧バージョンのconfig.json (OAuth設定入り) を読んでもエラーにしない
        let dir = temp_dir("legacy");
        fs::write(
            dir.join("config.json"),
            r#"{"instagram_app_id":"app123","oauth_redirect_port":8917,"polling_interval_secs":45}"#,
        )
        .unwrap();
        let config = load_or_create(&dir).unwrap();
        assert_eq!(config.polling_interval_secs, 45);
    }

    #[test]
    fn returns_config_error_for_broken_json() {
        let dir = temp_dir("broken");
        fs::write(dir.join("config.json"), "{ broken").unwrap();
        assert!(matches!(load_or_create(&dir), Err(AppError::Config(_))));
    }

    #[test]
    fn rejects_unsupported_auth_mode() {
        let config = AppConfig {
            meta_auth_mode: "facebook_login".into(),
            ..AppConfig::default()
        };
        assert!(matches!(config.validate(), Err(AppError::Config(_))));
    }

    #[test]
    fn rejects_invalid_api_version() {
        for bad in ["latest", "25.0", "vlatest", ""] {
            let config = AppConfig {
                meta_graph_api_version: bad.into(),
                ..AppConfig::default()
            };
            assert!(
                matches!(config.validate(), Err(AppError::Config(_))),
                "{} は拒否されるべき",
                bad
            );
        }
    }

    #[test]
    fn media_allowlist_empty_allows_all() {
        let config = AppConfig::default();
        assert!(config.is_media_allowed("any-media"));
    }

    #[test]
    fn media_allowlist_restricts_to_listed_ids() {
        let config = AppConfig {
            allowed_media_ids: vec!["m1".into(), "m2".into()],
            ..AppConfig::default()
        };
        assert!(config.is_media_allowed("m1"));
        assert!(!config.is_media_allowed("m3"));
    }
}
