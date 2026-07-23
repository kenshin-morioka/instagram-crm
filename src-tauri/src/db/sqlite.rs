use std::path::Path;
use std::sync::{Mutex, MutexGuard};

use rusqlite::{params, Connection, OptionalExtension};

use crate::error::{AppError, AppResult};

/// settingsテーブルのキー
pub mod keys {
    pub const REPLY_TEXT: &str = "reply_text";
    pub const POLLING_INTERVAL_SECS: &str = "polling_interval_secs";
    pub const LAST_RUN_AT: &str = "last_run_at";
    /// kill switch: "true" の間は新規送信を行わない
    pub const SENDING_PAUSED: &str = "sending_paused";
    /// ドライラン: UIから切り替えた値。未設定なら設定ファイルの初期値を使う
    pub const DRY_RUN: &str = "dry_run";
    /// 初回起動時の利用条件への同意日時 (RFC 3339)。未設定なら未同意
    pub const TERMS_ACCEPTED_AT: &str = "terms_accepted_at";
    /// 1周期で取得する自分のメディア件数: UIから保存した値。未設定なら設定ファイルの初期値
    pub const MEDIA_FETCH_LIMIT: &str = "media_fetch_limit";
    /// 何時間前までのコメントを返信対象とするか: UIから保存した値。未設定なら設定ファイルの初期値
    pub const COMMENT_LOOKBACK_HOURS: &str = "comment_lookback_hours";
    /// 直近の成功した周期の集計 (UI表示用。例: "コメント32件を確認 / 返信2件")
    pub const LAST_CYCLE_SUMMARY: &str = "last_cycle_summary";
}

/// 返信処理の状態
pub mod reply_status {
    /// 送信処理中 (この状態のままアプリが落ちた場合は結果不明としてunknownへ移す)
    pub const PROCESSING: &str = "processing";
    /// 一時エラーにより次周期で再試行する
    pub const QUEUED: &str = "queued";
    pub const SUCCEEDED: &str = "succeeded";
    /// 恒久エラー。自動再試行しない
    pub const FAILED: &str = "failed";
    /// 送信したが結果不明 (タイムアウト等)。二重返信防止のため自動再送しない
    pub const UNKNOWN: &str = "unknown";
    /// ドライランで対象と判定された (送信はしていない)
    pub const DRY_RUN: &str = "dry_run";
}

pub struct Db {
    conn: Mutex<Connection>,
}

impl Db {
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::from_connection(Connection::open(path)?)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> AppResult<Self> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> AppResult<Self> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS comments (
                comment_id TEXT PRIMARY KEY,
                media_id   TEXT,
                replied_at DATETIME
            );
            CREATE TABLE IF NOT EXISTS settings (
                key   TEXT PRIMARY KEY,
                value TEXT
            );",
        )?;
        Self::migrate(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// 旧スキーマ (comment_id/media_id/replied_atのみ) へ返信状態カラムを追加する
    fn migrate(conn: &Connection) -> AppResult<()> {
        let existing: Vec<String> = conn
            .prepare("SELECT name FROM pragma_table_info('comments')")?
            .query_map([], |row| row.get(0))?
            .collect::<Result<_, _>>()?;

        let wanted: &[(&str, &str)] = &[
            ("ig_user_id", "TEXT"),
            ("action_type", "TEXT"),
            ("reply_text_hash", "TEXT"),
            ("status", "TEXT"),
            ("reply_id", "TEXT"),
            ("http_status", "INTEGER"),
            ("meta_error_code", "INTEGER"),
            ("fbtrace_id", "TEXT"),
            ("attempt_count", "INTEGER NOT NULL DEFAULT 0"),
            ("started_at", "DATETIME"),
            ("completed_at", "DATETIME"),
        ];
        for (name, ddl) in wanted {
            if !existing.iter().any(|c| c == name) {
                conn.execute(
                    &format!("ALTER TABLE comments ADD COLUMN {} {}", name, ddl),
                    [],
                )?;
            }
        }
        // 旧スキーマで記録済みの行 (status NULL) は返信成功済みとして扱う
        conn.execute(
            "UPDATE comments SET status = ?1 WHERE status IS NULL AND replied_at IS NOT NULL",
            params![reply_status::SUCCEEDED],
        )?;
        Ok(())
    }

    fn conn(&self) -> AppResult<MutexGuard<'_, Connection>> {
        self.conn
            .lock()
            .map_err(|_| AppError::Other("DBコネクションのロックに失敗しました".into()))
    }

    /// このコメントが処理済みか。
    /// queued (再試行待ち) と dry_run (ドライラン解除後に返信可能にする) は未処理扱い
    pub fn is_processed(&self, comment_id: &str) -> AppResult<bool> {
        let conn = self.conn()?;
        let status: Option<Option<String>> = conn
            .query_row(
                "SELECT status FROM comments WHERE comment_id = ?1",
                params![comment_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(match status {
            None => false,
            Some(s) => !matches!(
                s.as_deref(),
                Some(reply_status::QUEUED) | Some(reply_status::DRY_RUN)
            ),
        })
    }

    /// このコメントが再試行待ち (queued) か。
    /// 再試行では前回の送信が実は成功していた可能性があるため、
    /// 送信前に返信一覧を確認する判断に使う
    pub fn is_queued(&self, comment_id: &str) -> AppResult<bool> {
        let conn = self.conn()?;
        let status: Option<Option<String>> = conn
            .query_row(
                "SELECT status FROM comments WHERE comment_id = ?1",
                params![comment_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(matches!(
            status,
            Some(Some(s)) if s == reply_status::QUEUED
        ))
    }

    /// 送信前にコメントを処理中として確保する。
    /// 未記録・queued・dry_runの場合のみ確保でき、成功時にtrueを返す (二重送信防止)
    pub fn try_begin_reply(
        &self,
        comment_id: &str,
        media_id: &str,
        ig_user_id: &str,
        reply_text_hash: &str,
    ) -> AppResult<bool> {
        let conn = self.conn()?;
        let changed = conn.execute(
            "INSERT INTO comments
                (comment_id, media_id, ig_user_id, action_type, reply_text_hash,
                 status, attempt_count, started_at)
             VALUES (?1, ?2, ?3, 'public_reply', ?4, ?5, 1, datetime('now'))
             ON CONFLICT(comment_id) DO UPDATE SET
                status = ?5,
                reply_text_hash = ?4,
                attempt_count = attempt_count + 1,
                started_at = datetime('now')
             WHERE comments.status IN (?6, ?7)",
            params![
                comment_id,
                media_id,
                ig_user_id,
                reply_text_hash,
                reply_status::PROCESSING,
                reply_status::QUEUED,
                reply_status::DRY_RUN
            ],
        )?;
        Ok(changed > 0)
    }

    pub fn complete_reply_success(
        &self,
        comment_id: &str,
        reply_id: &str,
        http_status: u16,
    ) -> AppResult<()> {
        self.finish(
            comment_id,
            reply_status::SUCCEEDED,
            Some(reply_id),
            Some(http_status),
            None,
            None,
        )
    }

    pub fn complete_reply_failure(
        &self,
        comment_id: &str,
        http_status: Option<u16>,
        meta_error_code: Option<i64>,
        fbtrace_id: Option<&str>,
    ) -> AppResult<()> {
        self.finish(
            comment_id,
            reply_status::FAILED,
            None,
            http_status,
            meta_error_code,
            fbtrace_id,
        )
    }

    /// 送信結果不明 (タイムアウト等)。二重返信を避けるため自動再送対象にしない
    pub fn complete_reply_unknown(&self, comment_id: &str) -> AppResult<()> {
        self.finish(comment_id, reply_status::UNKNOWN, None, None, None, None)
    }

    /// 一時エラーのため次周期で再試行できるよう戻す
    pub fn requeue_reply(&self, comment_id: &str) -> AppResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE comments SET status = ?1 WHERE comment_id = ?2 AND status = ?3",
            params![reply_status::QUEUED, comment_id, reply_status::PROCESSING],
        )?;
        Ok(())
    }

    /// 一時エラーの再試行キューへ戻す。ただし試行回数が上限に達していたら
    /// failedにして打ち切る (恒常的に一時エラーを返すコメントへのAPI消費を止める)。
    /// 再試行キューへ戻せたらtrue、打ち切ったらfalseを返す
    pub fn requeue_reply_or_give_up(
        &self,
        comment_id: &str,
        max_attempts: i64,
    ) -> AppResult<bool> {
        let conn = self.conn()?;
        let requeued = conn.execute(
            "UPDATE comments SET status = ?1
             WHERE comment_id = ?2 AND status = ?3 AND attempt_count < ?4",
            params![
                reply_status::QUEUED,
                comment_id,
                reply_status::PROCESSING,
                max_attempts
            ],
        )?;
        if requeued > 0 {
            return Ok(true);
        }
        conn.execute(
            "UPDATE comments SET status = ?1, completed_at = datetime('now')
             WHERE comment_id = ?2 AND status = ?3",
            params![reply_status::FAILED, comment_id, reply_status::PROCESSING],
        )?;
        Ok(false)
    }

    /// ドライランで対象と判定されたことを記録する。
    /// 新規検出時のみtrueを返す (毎周期の重複ログを防ぐ)
    pub fn record_dry_run(
        &self,
        comment_id: &str,
        media_id: &str,
        ig_user_id: &str,
    ) -> AppResult<bool> {
        let conn = self.conn()?;
        let changed = conn.execute(
            "INSERT OR IGNORE INTO comments
                (comment_id, media_id, ig_user_id, action_type, status, started_at)
             VALUES (?1, ?2, ?3, 'public_reply', ?4, datetime('now'))",
            params![comment_id, media_id, ig_user_id, reply_status::DRY_RUN],
        )?;
        Ok(changed > 0)
    }

    /// 前回起動時にprocessingのまま残った行を結果不明へ移す (クラッシュ対策)
    pub fn mark_stale_processing_as_unknown(&self) -> AppResult<usize> {
        let conn = self.conn()?;
        let changed = conn.execute(
            "UPDATE comments SET status = ?1, completed_at = datetime('now')
             WHERE status = ?2",
            params![reply_status::UNKNOWN, reply_status::PROCESSING],
        )?;
        Ok(changed)
    }

    fn finish(
        &self,
        comment_id: &str,
        status: &str,
        reply_id: Option<&str>,
        http_status: Option<u16>,
        meta_error_code: Option<i64>,
        fbtrace_id: Option<&str>,
    ) -> AppResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "UPDATE comments SET
                status = ?1,
                reply_id = ?2,
                http_status = ?3,
                meta_error_code = ?4,
                fbtrace_id = ?5,
                replied_at = CASE WHEN ?1 = 'succeeded' THEN datetime('now') ELSE replied_at END,
                completed_at = datetime('now')
             WHERE comment_id = ?6",
            params![
                status,
                reply_id,
                http_status,
                meta_error_code,
                fbtrace_id,
                comment_id
            ],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> AppResult<Option<String>> {
        let conn = self.conn()?;
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> AppResult<()> {
        let conn = self.conn()?;
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    #[cfg(test)]
    pub fn status_of(&self, comment_id: &str) -> AppResult<Option<String>> {
        let conn = self.conn()?;
        Ok(conn
            .query_row(
                "SELECT status FROM comments WHERE comment_id = ?1",
                params![comment_id],
                |row| row.get(0),
            )
            .optional()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db() -> Db {
        Db::open_in_memory().unwrap()
    }

    #[test]
    fn is_processed_returns_false_for_unknown_comment() {
        assert!(!db().is_processed("c1").unwrap());
    }

    #[test]
    fn try_begin_reply_claims_only_once() {
        let db = db();
        assert!(db.try_begin_reply("c1", "m1", "u1", "hash").unwrap());
        // 処理中・完了済みは再確保できない (同一コメントへの二重送信防止)
        assert!(!db.try_begin_reply("c1", "m1", "u1", "hash").unwrap());
        db.complete_reply_success("c1", "r1", 200).unwrap();
        assert!(!db.try_begin_reply("c1", "m1", "u1", "hash").unwrap());
    }

    #[test]
    fn success_marks_comment_processed() {
        let db = db();
        db.try_begin_reply("c1", "m1", "u1", "hash").unwrap();
        db.complete_reply_success("c1", "r1", 200).unwrap();
        assert!(db.is_processed("c1").unwrap());
        assert_eq!(db.status_of("c1").unwrap().as_deref(), Some("succeeded"));
    }

    #[test]
    fn failure_is_terminal_and_not_retried() {
        let db = db();
        db.try_begin_reply("c1", "m1", "u1", "hash").unwrap();
        db.complete_reply_failure("c1", Some(403), Some(10), Some("trace1"))
            .unwrap();
        assert!(db.is_processed("c1").unwrap());
        assert!(!db.try_begin_reply("c1", "m1", "u1", "hash").unwrap());
    }

    #[test]
    fn unknown_outcome_is_never_auto_retried() {
        let db = db();
        db.try_begin_reply("c1", "m1", "u1", "hash").unwrap();
        db.complete_reply_unknown("c1").unwrap();
        assert!(db.is_processed("c1").unwrap());
        assert!(
            !db.try_begin_reply("c1", "m1", "u1", "hash").unwrap(),
            "結果不明の返信はblind retryしない"
        );
    }

    #[test]
    fn requeued_comment_can_be_claimed_again() {
        let db = db();
        db.try_begin_reply("c1", "m1", "u1", "hash").unwrap();
        db.requeue_reply("c1").unwrap();
        assert!(!db.is_processed("c1").unwrap(), "queuedは再試行対象");
        assert!(db.try_begin_reply("c1", "m1", "u1", "hash").unwrap());
    }

    #[test]
    fn is_queued_only_for_requeued_comment() {
        let db = db();
        assert!(!db.is_queued("c1").unwrap(), "未記録はqueuedではない");
        db.try_begin_reply("c1", "m1", "u1", "hash").unwrap();
        assert!(!db.is_queued("c1").unwrap(), "processingはqueuedではない");
        db.requeue_reply("c1").unwrap();
        assert!(db.is_queued("c1").unwrap());
        db.try_begin_reply("c1", "m1", "u1", "hash").unwrap();
        db.complete_reply_success("c1", "r1", 200).unwrap();
        assert!(!db.is_queued("c1").unwrap(), "succeededはqueuedではない");
    }

    #[test]
    fn requeue_or_give_up_requeues_under_attempt_limit() {
        let db = db();
        db.try_begin_reply("c1", "m1", "u1", "hash").unwrap();
        assert!(db.requeue_reply_or_give_up("c1", 5).unwrap());
        assert_eq!(db.status_of("c1").unwrap().as_deref(), Some("queued"));
    }

    #[test]
    fn requeue_or_give_up_fails_permanently_at_attempt_limit() {
        let db = db();
        for attempt in 1..=5 {
            assert!(db.try_begin_reply("c1", "m1", "u1", "hash").unwrap());
            let requeued = db.requeue_reply_or_give_up("c1", 5).unwrap();
            if attempt < 5 {
                assert!(requeued, "{}回目はまだ再試行できる", attempt);
            } else {
                assert!(!requeued, "上限到達で打ち切られる");
            }
        }
        assert_eq!(db.status_of("c1").unwrap().as_deref(), Some("failed"));
        assert!(
            !db.try_begin_reply("c1", "m1", "u1", "hash").unwrap(),
            "打ち切り後は再確保できない"
        );
    }

    #[test]
    fn stale_processing_rows_become_unknown_on_startup() {
        let db = db();
        db.try_begin_reply("c1", "m1", "u1", "hash").unwrap();
        let changed = db.mark_stale_processing_as_unknown().unwrap();
        assert_eq!(changed, 1);
        assert_eq!(db.status_of("c1").unwrap().as_deref(), Some("unknown"));
    }

    #[test]
    fn dry_run_record_reports_new_detection_only_once() {
        let db = db();
        assert!(db.record_dry_run("c1", "m1", "u1").unwrap());
        assert!(!db.record_dry_run("c1", "m1", "u1").unwrap());
    }

    #[test]
    fn dry_run_comment_becomes_replyable_after_dry_run_disabled() {
        // ドライラン期間中に検出したコメントは、解除後に返信対象へ戻る
        let db = db();
        db.record_dry_run("c1", "m1", "u1").unwrap();
        assert!(!db.is_processed("c1").unwrap());
        assert!(db.try_begin_reply("c1", "m1", "u1", "hash").unwrap());
    }

    #[test]
    fn migrates_legacy_rows_as_succeeded() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE comments (
                comment_id TEXT PRIMARY KEY,
                media_id   TEXT,
                replied_at DATETIME
            );
            INSERT INTO comments VALUES ('old1', 'm1', datetime('now'));",
        )
        .unwrap();
        let db = Db::from_connection(conn).unwrap();
        assert!(db.is_processed("old1").unwrap());
        assert_eq!(db.status_of("old1").unwrap().as_deref(), Some("succeeded"));
    }

    #[test]
    fn get_setting_returns_none_for_unknown_key() {
        assert_eq!(db().get_setting("nope").unwrap(), None);
    }

    #[test]
    fn set_setting_inserts_and_updates() {
        let db = db();
        db.set_setting("k", "v1").unwrap();
        assert_eq!(db.get_setting("k").unwrap(), Some("v1".to_string()));
        db.set_setting("k", "v2").unwrap();
        assert_eq!(db.get_setting("k").unwrap(), Some("v2".to_string()));
    }
}
