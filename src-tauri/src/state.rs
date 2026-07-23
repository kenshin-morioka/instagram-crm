use std::sync::RwLock;

use crate::api::instagram::InstagramClient;
use crate::config::AppConfig;
use crate::db::{keys, Db};
use crate::models::{ConnectionStatus, TokenInfo};

/// 下限を30秒にしているのは、Instagram Graph APIのレート制限
/// (アプリ・アカウント単位の時間あたり呼び出し数) に達しにくくするため
pub const MIN_POLLING_INTERVAL_SECS: u64 = 30;
/// 上限を12時間にしているのは、コメントの返信対象が既定で直近24時間
/// (comment_lookback_hours) のため、それを超える間隔だと取りこぼすから
pub const MAX_POLLING_INTERVAL_SECS: u64 = 12 * 60 * 60;

/// 1周期で取得するメディア件数の範囲。上限はAPI呼び出し数の急増を防ぐため
/// (メディア1件につきコメント取得APIが最低1回走る)
pub const MIN_MEDIA_FETCH_LIMIT: u32 = 1;
pub const MAX_MEDIA_FETCH_LIMIT: u32 = 25;

/// コメント対象期間 (何時間前まで) の範囲。上限7日は、それより古いコメントへの
/// 突然の定型返信がスパム的に映るのを避けるため
pub const MIN_LOOKBACK_HOURS: i64 = 1;
pub const MAX_LOOKBACK_HOURS: i64 = 7 * 24;

/// 1リールあたりで確認するコメント件数上限の範囲。
/// 下限50はAPIの1ページ分。上限1000はコメント総数の多いリールで
/// 毎周期のAPI消費が際限なく増えるのを防ぐため
pub const MIN_COMMENT_FETCH_LIMIT: u32 = 50;
pub const MAX_COMMENT_FETCH_LIMIT: u32 = 1000;

pub struct AppState {
    pub config: AppConfig,
    pub db: Db,
    pub ig: InstagramClient,
    token: RwLock<Option<TokenInfo>>,
    status: RwLock<ConnectionStatus>,
    /// ポーリング中の進捗表示 (UI用)。周期の外ではNone
    cycle_progress: RwLock<Option<String>>,
    /// 一時エラーが連続していて自動再試行中か (UI警告表示用)
    connection_issue: RwLock<bool>,
    /// 直近の周期でコメント取得がページ上限で打ち切られたか (返信漏れ可能性の通知用)
    fetch_truncated: RwLock<bool>,
}

impl AppState {
    pub fn new(config: AppConfig, db: Db, token: Option<TokenInfo>) -> Self {
        let status = match &token {
            Some(t) if t.is_expired() => ConnectionStatus::NeedsReauth,
            Some(_) => ConnectionStatus::Connected,
            None => ConnectionStatus::NotConnected,
        };
        let ig = InstagramClient::new(&config.meta_graph_api_version);
        Self {
            config,
            db,
            ig,
            token: RwLock::new(token),
            status: RwLock::new(status),
            cycle_progress: RwLock::new(None),
            connection_issue: RwLock::new(false),
            fetch_truncated: RwLock::new(false),
        }
    }

    pub fn token(&self) -> Option<TokenInfo> {
        // RwLockのpoisoningは書き込み中のpanicでしか起きないため、
        // 発生時はトークンなし扱いにして再ログインを促す
        self.token.read().ok().and_then(|t| t.clone())
    }

    pub fn set_token(&self, token: Option<TokenInfo>) {
        if let Ok(mut guard) = self.token.write() {
            *guard = token;
        }
    }

    pub fn status(&self) -> ConnectionStatus {
        self.status
            .read()
            .map(|s| *s)
            .unwrap_or(ConnectionStatus::NotConnected)
    }

    pub fn set_status(&self, status: ConnectionStatus) {
        if let Ok(mut guard) = self.status.write() {
            *guard = status;
        }
    }

    /// UIで保存された値 (SQLite) を優先し、なければ設定ファイルの初期値を使う
    pub fn polling_interval_secs(&self) -> u64 {
        let saved = self
            .db
            .get_setting(keys::POLLING_INTERVAL_SECS)
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u64>().ok());
        saved
            .unwrap_or(self.config.polling_interval_secs)
            .clamp(MIN_POLLING_INTERVAL_SECS, MAX_POLLING_INTERVAL_SECS)
    }

    /// 1周期で取得するメディア件数。UIで保存された値 (SQLite) を優先し、
    /// なければ設定ファイルの初期値を使う
    pub fn media_fetch_limit(&self) -> u32 {
        self.db
            .get_setting(keys::MEDIA_FETCH_LIMIT)
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(self.config.media_fetch_limit)
            .clamp(MIN_MEDIA_FETCH_LIMIT, MAX_MEDIA_FETCH_LIMIT)
    }

    /// 何時間前までのコメントを返信対象とするか。UIで保存された値 (SQLite) を優先し、
    /// なければ設定ファイルの初期値を使う
    pub fn comment_lookback_hours(&self) -> i64 {
        self.db
            .get_setting(keys::COMMENT_LOOKBACK_HOURS)
            .ok()
            .flatten()
            .and_then(|v| v.parse::<i64>().ok())
            .unwrap_or(self.config.comment_lookback_hours)
            .clamp(MIN_LOOKBACK_HOURS, MAX_LOOKBACK_HOURS)
    }

    /// 1リールあたりで確認するコメント件数の上限。UIで保存された値 (SQLite) を優先し、
    /// なければ設定ファイルの初期値を使う
    pub fn comment_fetch_limit(&self) -> u32 {
        self.db
            .get_setting(keys::COMMENT_FETCH_LIMIT)
            .ok()
            .flatten()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(self.config.comment_fetch_limit)
            .clamp(MIN_COMMENT_FETCH_LIMIT, MAX_COMMENT_FETCH_LIMIT)
    }

    /// ポーリング中の進捗表示 (UI用)
    pub fn cycle_progress(&self) -> Option<String> {
        self.cycle_progress.read().ok().and_then(|p| p.clone())
    }

    pub fn set_cycle_progress(&self, progress: Option<String>) {
        if let Ok(mut guard) = self.cycle_progress.write() {
            *guard = progress;
        }
    }

    /// 一時エラーが連続していて自動再試行中か (UI警告表示用)
    pub fn connection_issue(&self) -> bool {
        self.connection_issue.read().map(|v| *v).unwrap_or(false)
    }

    pub fn set_connection_issue(&self, issue: bool) {
        if let Ok(mut guard) = self.connection_issue.write() {
            *guard = issue;
        }
    }

    /// 直近の周期でコメント取得がページ上限で打ち切られたか
    pub fn fetch_truncated(&self) -> bool {
        self.fetch_truncated.read().map(|v| *v).unwrap_or(false)
    }

    pub fn set_fetch_truncated(&self, truncated: bool) {
        if let Ok(mut guard) = self.fetch_truncated.write() {
            *guard = truncated;
        }
    }

    /// 直近の成功した周期の集計 (UI表示用)
    pub fn last_cycle_summary(&self) -> Option<String> {
        self.db.get_setting(keys::LAST_CYCLE_SUMMARY).ok().flatten()
    }

    /// 返信文。未設定なら空文字を返す (デフォルト文は持たず、未設定時は送信しない)
    pub fn reply_text(&self) -> String {
        self.db
            .get_setting(keys::REPLY_TEXT)
            .ok()
            .flatten()
            .filter(|v| !v.trim().is_empty())
            .unwrap_or_default()
    }

    pub fn last_run_at(&self) -> Option<String> {
        self.db.get_setting(keys::LAST_RUN_AT).ok().flatten()
    }

    /// kill switch: trueの間は新規送信を行わない
    pub fn sending_paused(&self) -> bool {
        self.db
            .get_setting(keys::SENDING_PAUSED)
            .ok()
            .flatten()
            .as_deref()
            == Some("true")
    }

    pub fn set_sending_paused(&self, paused: bool) -> crate::error::AppResult<()> {
        self.db
            .set_setting(keys::SENDING_PAUSED, if paused { "true" } else { "false" })
    }

    /// UIで切り替えた値 (SQLite) を優先し、なければ設定ファイルの初期値を使う
    pub fn dry_run(&self) -> bool {
        self.db
            .get_setting(keys::DRY_RUN)
            .ok()
            .flatten()
            .and_then(|v| v.parse::<bool>().ok())
            .unwrap_or(self.config.dry_run)
    }

    pub fn set_dry_run(&self, enabled: bool) -> crate::error::AppResult<()> {
        self.db
            .set_setting(keys::DRY_RUN, if enabled { "true" } else { "false" })
    }

    /// 初回起動時の利用条件に同意済みか。値が読めない場合は未同意扱い (安全側)
    pub fn terms_accepted(&self) -> bool {
        self.db
            .get_setting(keys::TERMS_ACCEPTED_AT)
            .ok()
            .flatten()
            .is_some_and(|v| !v.trim().is_empty())
    }

    pub fn set_terms_accepted(&self, accepted_at: &str) -> crate::error::AppResult<()> {
        self.db.set_setting(keys::TERMS_ACCEPTED_AT, accepted_at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn state_with(token: Option<TokenInfo>) -> AppState {
        AppState::new(
            AppConfig::default(),
            Db::open_in_memory().expect("in-memory db"),
            token,
        )
    }

    fn token_expiring_in(secs: i64) -> TokenInfo {
        TokenInfo {
            access_token: "t".into(),
            user_id: "u".into(),
            expires_at: Utc::now().timestamp() + secs,
        }
    }

    #[test]
    fn status_is_not_connected_without_token() {
        assert_eq!(state_with(None).status(), ConnectionStatus::NotConnected);
    }

    #[test]
    fn status_is_connected_with_valid_token() {
        assert_eq!(
            state_with(Some(token_expiring_in(3600))).status(),
            ConnectionStatus::Connected
        );
    }

    #[test]
    fn status_is_needs_reauth_with_expired_token() {
        assert_eq!(
            state_with(Some(token_expiring_in(-1))).status(),
            ConnectionStatus::NeedsReauth
        );
    }

    #[test]
    fn set_token_and_set_status_update_state() {
        let state = state_with(None);
        state.set_token(Some(token_expiring_in(3600)));
        state.set_status(ConnectionStatus::Connected);
        assert!(state.token().is_some());
        assert_eq!(state.status(), ConnectionStatus::Connected);
    }

    #[test]
    fn polling_interval_falls_back_to_config_without_db_value() {
        let state = state_with(None);
        assert_eq!(state.polling_interval_secs(), 30);
    }

    #[test]
    fn polling_interval_prefers_db_value() {
        let state = state_with(None);
        state
            .db
            .set_setting(keys::POLLING_INTERVAL_SECS, "60")
            .unwrap();
        assert_eq!(state.polling_interval_secs(), 60);
    }

    #[test]
    fn polling_interval_falls_back_when_db_value_is_not_numeric() {
        let state = state_with(None);
        state
            .db
            .set_setting(keys::POLLING_INTERVAL_SECS, "abc")
            .unwrap();
        assert_eq!(state.polling_interval_secs(), 30);
    }

    #[test]
    fn polling_interval_is_clamped_to_bounds() {
        let state = state_with(None);
        state.db.set_setting(keys::POLLING_INTERVAL_SECS, "1").unwrap();
        assert_eq!(state.polling_interval_secs(), MIN_POLLING_INTERVAL_SECS);
        state
            .db
            .set_setting(keys::POLLING_INTERVAL_SECS, "99999")
            .unwrap();
        assert_eq!(state.polling_interval_secs(), MAX_POLLING_INTERVAL_SECS);
    }

    #[test]
    fn polling_interval_accepts_boundary_values() {
        let state = state_with(None);
        state
            .db
            .set_setting(keys::POLLING_INTERVAL_SECS, &MIN_POLLING_INTERVAL_SECS.to_string())
            .unwrap();
        assert_eq!(state.polling_interval_secs(), MIN_POLLING_INTERVAL_SECS);
        state
            .db
            .set_setting(keys::POLLING_INTERVAL_SECS, &MAX_POLLING_INTERVAL_SECS.to_string())
            .unwrap();
        assert_eq!(state.polling_interval_secs(), MAX_POLLING_INTERVAL_SECS);
    }

    #[test]
    fn reply_text_is_empty_when_missing_or_blank() {
        let state = state_with(None);
        assert_eq!(state.reply_text(), "");
        state.db.set_setting(keys::REPLY_TEXT, "   ").unwrap();
        assert_eq!(state.reply_text(), "");
    }

    #[test]
    fn reply_text_returns_saved_value() {
        let state = state_with(None);
        state.db.set_setting(keys::REPLY_TEXT, "こんにちは！").unwrap();
        assert_eq!(state.reply_text(), "こんにちは！");
    }

    #[test]
    fn sending_paused_defaults_to_false_and_toggles() {
        let state = state_with(None);
        assert!(!state.sending_paused());
        state.set_sending_paused(true).unwrap();
        assert!(state.sending_paused());
        state.set_sending_paused(false).unwrap();
        assert!(!state.sending_paused());
    }

    #[test]
    fn dry_run_defaults_to_config_value_and_ui_override_wins() {
        let state = state_with(None);
        assert!(state.dry_run(), "設定ファイル初期値 (true) が使われる");
        state.set_dry_run(false).unwrap();
        assert!(!state.dry_run(), "UIでの切り替えが優先される");
        state.set_dry_run(true).unwrap();
        assert!(state.dry_run());
    }

    #[test]
    fn dry_run_falls_back_to_config_when_db_value_is_invalid() {
        let state = state_with(None);
        state.db.set_setting(keys::DRY_RUN, "yes").unwrap();
        assert!(state.dry_run());
    }

    #[test]
    fn terms_accepted_defaults_to_false_and_persists() {
        let state = state_with(None);
        assert!(!state.terms_accepted(), "初期状態は未同意であること");
        state.set_terms_accepted("2026-07-24T00:00:00Z").unwrap();
        assert!(state.terms_accepted());
    }

    #[test]
    fn terms_accepted_treats_blank_value_as_not_accepted() {
        let state = state_with(None);
        state.db.set_setting(keys::TERMS_ACCEPTED_AT, "  ").unwrap();
        assert!(!state.terms_accepted());
    }

    #[test]
    fn media_fetch_limit_falls_back_to_config_and_prefers_db() {
        let state = state_with(None);
        assert_eq!(state.media_fetch_limit(), 10, "設定ファイル初期値");
        state.db.set_setting(keys::MEDIA_FETCH_LIMIT, "5").unwrap();
        assert_eq!(state.media_fetch_limit(), 5, "UIで保存した値が優先");
    }

    #[test]
    fn media_fetch_limit_is_clamped_and_ignores_invalid() {
        let state = state_with(None);
        state.db.set_setting(keys::MEDIA_FETCH_LIMIT, "0").unwrap();
        assert_eq!(state.media_fetch_limit(), MIN_MEDIA_FETCH_LIMIT);
        state.db.set_setting(keys::MEDIA_FETCH_LIMIT, "999").unwrap();
        assert_eq!(state.media_fetch_limit(), MAX_MEDIA_FETCH_LIMIT);
        state.db.set_setting(keys::MEDIA_FETCH_LIMIT, "abc").unwrap();
        assert_eq!(state.media_fetch_limit(), 10, "不正値は初期値へフォールバック");
    }

    #[test]
    fn comment_lookback_hours_falls_back_to_config_and_prefers_db() {
        let state = state_with(None);
        assert_eq!(state.comment_lookback_hours(), 24, "設定ファイル初期値");
        state
            .db
            .set_setting(keys::COMMENT_LOOKBACK_HOURS, "48")
            .unwrap();
        assert_eq!(state.comment_lookback_hours(), 48, "UIで保存した値が優先");
    }

    #[test]
    fn comment_lookback_hours_is_clamped() {
        let state = state_with(None);
        state
            .db
            .set_setting(keys::COMMENT_LOOKBACK_HOURS, "0")
            .unwrap();
        assert_eq!(state.comment_lookback_hours(), MIN_LOOKBACK_HOURS);
        state
            .db
            .set_setting(keys::COMMENT_LOOKBACK_HOURS, "9999")
            .unwrap();
        assert_eq!(state.comment_lookback_hours(), MAX_LOOKBACK_HOURS);
    }

    #[test]
    fn comment_fetch_limit_falls_back_prefers_db_and_clamps() {
        let state = state_with(None);
        assert_eq!(state.comment_fetch_limit(), 200, "設定ファイル初期値");
        state
            .db
            .set_setting(keys::COMMENT_FETCH_LIMIT, "500")
            .unwrap();
        assert_eq!(state.comment_fetch_limit(), 500, "UIで保存した値が優先");
        state.db.set_setting(keys::COMMENT_FETCH_LIMIT, "1").unwrap();
        assert_eq!(state.comment_fetch_limit(), MIN_COMMENT_FETCH_LIMIT);
        state
            .db
            .set_setting(keys::COMMENT_FETCH_LIMIT, "99999")
            .unwrap();
        assert_eq!(state.comment_fetch_limit(), MAX_COMMENT_FETCH_LIMIT);
    }

    #[test]
    fn cycle_progress_and_flags_default_and_toggle() {
        let state = state_with(None);
        assert_eq!(state.cycle_progress(), None);
        assert!(!state.connection_issue());
        assert!(!state.fetch_truncated());

        state.set_cycle_progress(Some("取得中".into()));
        state.set_connection_issue(true);
        state.set_fetch_truncated(true);
        assert_eq!(state.cycle_progress(), Some("取得中".into()));
        assert!(state.connection_issue());
        assert!(state.fetch_truncated());

        state.set_cycle_progress(None);
        state.set_connection_issue(false);
        state.set_fetch_truncated(false);
        assert_eq!(state.cycle_progress(), None);
        assert!(!state.connection_issue());
        assert!(!state.fetch_truncated());
    }

    #[test]
    fn last_run_at_reflects_db_value() {
        let state = state_with(None);
        assert_eq!(state.last_run_at(), None);
        state
            .db
            .set_setting(keys::LAST_RUN_AT, "2026-07-23 12:34:56")
            .unwrap();
        assert_eq!(state.last_run_at(), Some("2026-07-23 12:34:56".into()));
    }
}
