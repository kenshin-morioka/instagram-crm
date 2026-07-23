use chrono::{DateTime, Duration, Local, Utc};
use sha2::{Digest, Sha256};

use crate::auth::{oauth, token_store};
use crate::config::AppConfig;
use crate::db::keys;
use crate::error::{AppError, AppResult};
use crate::models::{parse_ig_timestamp, Comment, TokenInfo};
use crate::state::AppState;

/// 有効期限がこの秒数を切ったらトークンをリフレッシュする (7日)
const TOKEN_REFRESH_THRESHOLD_SECS: i64 = 7 * 24 * 60 * 60;

/// 一時エラーでの再試行回数の上限。恒常的に一時エラーを返すコメントが
/// 毎周期API消費し続けるのを防ぎ、超過したらfailedで打ち切る
const MAX_REPLY_ATTEMPTS: i64 = 5;

#[derive(Debug, Default)]
pub struct CycleReport {
    pub fetched_comments: usize,
    pub replied: usize,
    pub failed: usize,
    pub unknown: usize,
    pub requeued: usize,
    pub dry_run_matched: usize,
}

impl CycleReport {
    pub fn has_activity(&self) -> bool {
        self.replied > 0
            || self.failed > 0
            || self.unknown > 0
            || self.requeued > 0
            || self.dry_run_matched > 0
    }
}

/// ポーリング1周期分の処理: リール取得→コメント取得→対象判定→返信→記録
pub async fn run_cycle(state: &AppState, http: &reqwest::Client) -> AppResult<CycleReport> {
    if state.sending_paused() {
        log::warn!("kill switchが有効のため、この周期の処理をスキップします");
        return Ok(CycleReport::default());
    }
    let reply_text = state.reply_text();
    if reply_text.is_empty() {
        log::info!("返信文が未設定のため、この周期の処理をスキップします");
        return Ok(CycleReport::default());
    }

    let Some(token) = state.token() else {
        return Ok(CycleReport::default());
    };
    if token.is_expired() {
        return Err(AppError::TokenExpired);
    }

    if let Some(pct) = state.ig.last_usage_pct() {
        if pct >= state.config.usage_pause_threshold_pct {
            // 完全にAPI呼び出しを止めると使用量の観測値が更新されず永久に停止するため、
            // 軽いプローブ (/me) だけ打って観測値を更新し、回復したら次周期から再開する
            match state.ig.get_me(&token.access_token).await {
                // トークン失効はNeedsReauth遷移と再連携通知が必要なため握りつぶさない
                Err(AppError::TokenExpired) => return Err(AppError::TokenExpired),
                Err(e) => log::warn!("使用量確認プローブに失敗: {}", e),
                Ok(_) => {}
            }
            log::warn!(
                "API使用量が{:.0}%に達したため、この周期の処理をスキップします (閾値{:.0}%)",
                pct,
                state.config.usage_pause_threshold_pct
            );
            return Ok(CycleReport::default());
        }
    }

    let token = maybe_refresh_token(state, http, token).await;

    let mut report = CycleReport::default();
    let lookback_limit = Utc::now() - Duration::hours(state.comment_lookback_hours());
    let reply_text_hash = text_hash(&reply_text);

    state.set_cycle_progress(Some("リール一覧を取得中...".into()));
    let media_list = state
        .ig
        .fetch_recent_media(&token.access_token, state.media_fetch_limit())
        .await?;
    let target_reels: Vec<_> = media_list
        .iter()
        .filter(|m| m.is_reel() && state.config.is_media_allowed(&m.id))
        .collect();
    let total_reels = target_reels.len();

    let mut any_truncated = false;
    for (index, media) in target_reels.into_iter().enumerate() {
        state.set_cycle_progress(Some(format!(
            "コメントを確認中... (リール {}/{})",
            index + 1,
            total_reels
        )));
        let page = state
            .ig
            .fetch_comments(&token.access_token, &media.id, state.comment_fetch_limit())
            .await?;
        let comments = page.comments;
        any_truncated |= page.truncated;
        log::info!("コメント取得: media_id={} 件数={}", media.id, comments.len());
        report.fetched_comments += comments.len();

        for comment in &comments {
            if !should_reply(&state.config, comment, &token, lookback_limit)
                || state.db.is_processed(&comment.id)?
            {
                continue;
            }

            if state.dry_run() {
                let newly_detected =
                    state
                        .db
                        .record_dry_run(&comment.id, &media.id, &token.user_id)?;
                if newly_detected {
                    report.dry_run_matched += 1;
                    log::info!(
                        "[DRY RUN] 返信対象を検出 (送信なし): comment_id={} media_id={}",
                        comment.id,
                        media.id
                    );
                }
                continue;
            }

            // kill switchを周期の途中でも次の送信前に反映する (周期開始時のみの
            // チェックだと、実行中の周期の残りコメントへ送信し続けてしまう)
            if state.sending_paused() {
                log::warn!("kill switchが有効になったため、この周期の残りの送信を中止します");
                return Ok(report);
            }

            // 再試行では前回の送信 (5xx等) が実は成功していた可能性があるため、
            // 冪等キーのないこのAPIでは送信前に自分の返信が付いていないか確認する
            if state.db.is_queued(&comment.id)? {
                match find_own_reply(state, &token, &comment.id).await {
                    Ok(Some(reply_id)) => {
                        state.db.complete_reply_success(&comment.id, &reply_id, 200)?;
                        report.replied += 1;
                        log::warn!(
                            "再試行前チェックで返信済みを検出 (二重返信を回避): comment_id={} reply_id={}",
                            comment.id,
                            reply_id
                        );
                        continue;
                    }
                    Ok(None) => {}
                    // 確認できないまま送信すると二重返信し得るため、この周期は見送る
                    Err(e) => {
                        log::warn!(
                            "返信済みチェックに失敗 (この周期は再試行を見送り): comment_id={} error={}",
                            comment.id,
                            e
                        );
                        continue;
                    }
                }
            }

            if !state
                .db
                .try_begin_reply(&comment.id, &media.id, &token.user_id, &reply_text_hash)?
            {
                continue;
            }
            if let Err(e) = send_reply(state, &token, comment, &reply_text, &mut report).await {
                // 中断までの部分集計を可視化してから伝播する (返信結果自体はDBに記録済み)
                log::warn!(
                    "周期を中断します (取得={} 返信={} 失敗={} 不明={} 再試行待ち={}): {}",
                    report.fetched_comments,
                    report.replied,
                    report.failed,
                    report.unknown,
                    report.requeued,
                    e
                );
                return Err(e);
            }
        }
    }

    // 打ち切りがあった周期はUIに「確認しきれていない可能性」を出し、
    // なく完了した周期で解除する
    state.set_fetch_truncated(any_truncated);
    if any_truncated {
        log::warn!("コメントが多く、この周期では一部を確認できていません (次周期で続きを確認)");
    }

    state.db.set_setting(
        keys::LAST_RUN_AT,
        &Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    )?;
    state
        .db
        .set_setting(keys::LAST_CYCLE_SUMMARY, &summarize(total_reels, &report))?;
    Ok(report)
}

/// UIの「実行状況」に出す1行サマリー
fn summarize(reels: usize, report: &CycleReport) -> String {
    let mut parts = vec![format!(
        "リール{}件・コメント{}件を確認",
        reels, report.fetched_comments
    )];
    if report.replied > 0 {
        parts.push(format!("返信{}件", report.replied));
    }
    if report.dry_run_matched > 0 {
        parts.push(format!("対象検出{}件 (ドライラン)", report.dry_run_matched));
    }
    if report.failed > 0 {
        parts.push(format!("失敗{}件", report.failed));
    }
    parts.join(" / ")
}

async fn send_reply(
    state: &AppState,
    token: &TokenInfo,
    comment: &Comment,
    reply_text: &str,
    report: &mut CycleReport,
) -> AppResult<()> {
    match state
        .ig
        .reply_to_comment(&token.access_token, &comment.id, reply_text)
        .await
    {
        Ok(reply_id) => {
            state
                .db
                .complete_reply_success(&comment.id, &reply_id, 200)?;
            report.replied += 1;
            log::info!(
                "コメント返信成功: comment_id={} reply_id={}",
                comment.id,
                reply_id
            );
            Ok(())
        }
        // 周期全体を止めるエラー: 再試行できるようqueuedへ戻して伝播する
        Err(e @ (AppError::RateLimited { .. } | AppError::TokenExpired)) => {
            state.db.requeue_reply(&comment.id)?;
            Err(e)
        }
        // 接続確立前の失敗はリクエストが送信されていないため、安全に再試行できる
        Err(e @ AppError::Http(_))
            if matches!(&e, AppError::Http(he) if he.is_connect()) =>
        {
            state.db.requeue_reply(&comment.id)?;
            report.requeued += 1;
            log::warn!(
                "コメント返信の接続に失敗 (次周期で再試行): comment_id={}",
                comment.id
            );
            Ok(())
        }
        // 送信後に結果を受け取れなかった場合 (タイムアウト・レスポンス読取り中の
        // 接続断等) は成否不明。blind retryによる二重返信を避けるため自動再送しない
        // (手動確認: RUNBOOK参照)
        Err(AppError::Http(e)) => {
            state.db.complete_reply_unknown(&comment.id)?;
            report.unknown += 1;
            log::error!(
                "コメント返信の結果不明 ({}): comment_id={} 自動再送しません",
                e,
                comment.id
            );
            Ok(())
        }
        Err(e) if e.is_retryable_transient() => {
            if state
                .db
                .requeue_reply_or_give_up(&comment.id, MAX_REPLY_ATTEMPTS)?
            {
                report.requeued += 1;
                log::warn!(
                    "コメント返信が一時エラー (次周期で再試行): comment_id={} error={}",
                    comment.id,
                    e
                );
            } else {
                report.failed += 1;
                log::error!(
                    "コメント返信の再試行が上限{}回に到達 (打ち切り): comment_id={} error={}",
                    MAX_REPLY_ATTEMPTS,
                    comment.id,
                    e
                );
            }
            Ok(())
        }
        // 恒久エラー (権限不足・不正ID・削除済み等): 記録して自動再試行しない
        Err(e) => {
            let (http_status, code, fbtrace) = match &e {
                AppError::Api {
                    http_status,
                    code,
                    fbtrace_id,
                    ..
                } => (Some(*http_status), Some(*code), fbtrace_id.clone()),
                _ => (None, None, None),
            };
            state
                .db
                .complete_reply_failure(&comment.id, http_status, code, fbtrace.as_deref())?;
            report.failed += 1;
            log::error!("コメント返信失敗: comment_id={} error={}", comment.id, e);
            Ok(())
        }
    }
}

/// コメントに自分 (token.user_id) の返信が既に付いていればそのreply_idを返す
async fn find_own_reply(
    state: &AppState,
    token: &TokenInfo,
    comment_id: &str,
) -> AppResult<Option<String>> {
    let replies = state
        .ig
        .fetch_replies(&token.access_token, comment_id)
        .await?;
    Ok(replies
        .into_iter()
        .find(|r| {
            r.from
                .as_ref()
                .is_some_and(|from| from.id == token.user_id)
        })
        .map(|r| r.id))
}

/// 設定に基づく返信対象判定 (DB照会を除く純粋関数)
fn should_reply(
    config: &AppConfig,
    comment: &Comment,
    token: &TokenInfo,
    lookback_limit: DateTime<Utc>,
) -> bool {
    // 自分自身のコメントに返信するとループの恐れがあるため除外する。
    // fromが取得できないコメントも投稿者を判定できないため安全側に倒して除外する
    match &comment.from {
        Some(from) if from.id != token.user_id => {}
        _ => return false,
    }
    if let Some(ts) = comment.timestamp.as_deref().and_then(parse_ig_timestamp) {
        if ts.with_timezone(&Utc) < lookback_limit {
            return false;
        }
    }

    let text = comment.text.as_deref().unwrap_or("").trim();
    if text.is_empty() {
        return false;
    }
    if text.chars().count() > config.max_comment_length {
        return false;
    }
    if !config.reply_keywords.is_empty() {
        let lowered = text.to_lowercase();
        if !config
            .reply_keywords
            .iter()
            .any(|kw| lowered.contains(&kw.to_lowercase()))
        {
            return false;
        }
    }
    true
}

fn text_hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// 有効期限が近ければリフレッシュする。失敗しても現行トークンで続行する
async fn maybe_refresh_token(
    state: &AppState,
    http: &reqwest::Client,
    token: TokenInfo,
) -> TokenInfo {
    if !token.expires_within_secs(TOKEN_REFRESH_THRESHOLD_SECS) {
        return token;
    }
    match oauth::refresh_token(http, &token).await {
        Ok(refreshed) => {
            if let Err(e) = token_store::save(&refreshed) {
                log::error!("リフレッシュ後のトークン保存に失敗: {}", e);
            }
            state.set_token(Some(refreshed.clone()));
            log::info!("アクセストークンをリフレッシュしました");
            refreshed
        }
        Err(e) => {
            // 発行24時間未満のトークン等でも失敗するため、期限内なら現行トークンで続行
            log::warn!("トークンのリフレッシュに失敗 (現行トークンで続行): {}", e);
            token
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::CommentFrom;

    const OWN_USER_ID: &str = "me-123";

    fn test_token() -> TokenInfo {
        TokenInfo {
            access_token: "t".into(),
            user_id: OWN_USER_ID.into(),
            expires_at: Utc::now().timestamp() + 3600,
        }
    }

    fn comment(from_id: Option<&str>, text: &str, timestamp: Option<String>) -> Comment {
        Comment {
            id: "c1".into(),
            text: Some(text.into()),
            timestamp,
            from: from_id.map(|id| CommentFrom { id: id.into() }),
        }
    }

    fn fresh_ts() -> Option<String> {
        Some(Utc::now().format("%Y-%m-%dT%H:%M:%S+0000").to_string())
    }

    fn old_ts() -> Option<String> {
        Some(
            (Utc::now() - Duration::hours(48))
                .format("%Y-%m-%dT%H:%M:%S+0000")
                .to_string(),
        )
    }

    fn lookback() -> DateTime<Utc> {
        Utc::now() - Duration::hours(24)
    }

    #[test]
    fn replies_to_new_comment_from_other_user() {
        let config = AppConfig::default();
        let target = comment(Some("other-1"), "いいですね", fresh_ts());
        assert!(should_reply(&config, &target, &test_token(), lookback()));
    }

    #[test]
    fn skips_own_comment_even_if_new() {
        let config = AppConfig::default();
        let target = comment(Some(OWN_USER_ID), "自分の返信", fresh_ts());
        assert!(!should_reply(&config, &target, &test_token(), lookback()));
    }

    #[test]
    fn skips_comment_older_than_lookback() {
        let config = AppConfig::default();
        let target = comment(Some("other-1"), "古いコメント", old_ts());
        assert!(!should_reply(&config, &target, &test_token(), lookback()));
    }

    #[test]
    fn skips_empty_or_missing_text() {
        let config = AppConfig::default();
        assert!(!should_reply(
            &config,
            &comment(Some("other-1"), "   ", fresh_ts()),
            &test_token(),
            lookback()
        ));
        let no_text = Comment {
            id: "c1".into(),
            text: None,
            timestamp: fresh_ts(),
            from: Some(CommentFrom {
                id: "other-1".into(),
            }),
        };
        assert!(!should_reply(&config, &no_text, &test_token(), lookback()));
    }

    #[test]
    fn skips_comment_exceeding_max_length() {
        let config = AppConfig {
            max_comment_length: 5,
            ..AppConfig::default()
        };
        assert!(should_reply(
            &config,
            &comment(Some("o"), "12345", fresh_ts()),
            &test_token(),
            lookback()
        ));
        assert!(!should_reply(
            &config,
            &comment(Some("o"), "123456", fresh_ts()),
            &test_token(),
            lookback()
        ));
    }

    #[test]
    fn keyword_filter_matches_case_insensitively() {
        let config = AppConfig {
            reply_keywords: vec!["レシピ".into(), "how".into()],
            ..AppConfig::default()
        };
        assert!(should_reply(
            &config,
            &comment(Some("o"), "レシピ教えて！", fresh_ts()),
            &test_token(),
            lookback()
        ));
        assert!(should_reply(
            &config,
            &comment(Some("o"), "HOW to make this?", fresh_ts()),
            &test_token(),
            lookback()
        ));
    }

    #[test]
    fn keyword_filter_rejects_non_matching_comment() {
        let config = AppConfig {
            reply_keywords: vec!["レシピ".into()],
            ..AppConfig::default()
        };
        assert!(!should_reply(
            &config,
            &comment(Some("o"), "かわいい", fresh_ts()),
            &test_token(),
            lookback()
        ));
    }

    #[test]
    fn no_keyword_filter_allows_any_text() {
        let config = AppConfig::default();
        assert!(should_reply(
            &config,
            &comment(Some("o"), "なんでもコメント", fresh_ts()),
            &test_token(),
            lookback()
        ));
    }

    #[test]
    fn skips_when_from_is_missing() {
        // 投稿者を判定できないコメントは安全側に倒して返信しない
        let config = AppConfig::default();
        let target = Comment {
            id: "c1".into(),
            text: Some("hi".into()),
            timestamp: fresh_ts(),
            from: None,
        };
        assert!(!should_reply(&config, &target, &test_token(), lookback()));
    }

    #[test]
    fn replies_when_timestamp_is_missing_or_unparsable() {
        let config = AppConfig::default();
        assert!(should_reply(
            &config,
            &comment(Some("o"), "hi", None),
            &test_token(),
            lookback()
        ));
        assert!(should_reply(
            &config,
            &comment(Some("o"), "hi", Some("not a date".into())),
            &test_token(),
            lookback()
        ));
    }

    #[tokio::test]
    async fn run_cycle_skips_when_reply_text_is_unset() {
        // 返信文が未設定の間はAPI呼び出しに到達せず何もしない
        let state = AppState::new(
            AppConfig::default(),
            crate::db::Db::open_in_memory().expect("in-memory db"),
            Some(test_token()),
        );
        let http = crate::api::instagram::http_client();
        let report = run_cycle(&state, &http).await.expect("skipされること");
        assert!(!report.has_activity());
    }

    #[test]
    fn summarize_reports_counts_compactly() {
        let mut report = CycleReport::default();
        report.fetched_comments = 12;
        assert_eq!(summarize(3, &report), "リール3件・コメント12件を確認");

        report.replied = 2;
        report.dry_run_matched = 1;
        report.failed = 1;
        assert_eq!(
            summarize(3, &report),
            "リール3件・コメント12件を確認 / 返信2件 / 対象検出1件 (ドライラン) / 失敗1件"
        );
    }

    #[test]
    fn text_hash_is_stable_and_distinct() {
        assert_eq!(text_hash("a"), text_hash("a"));
        assert_ne!(text_hash("a"), text_hash("b"));
        assert_eq!(text_hash("a").len(), 64);
    }
}
