use chrono::Utc;
use serde::Serialize;
use tauri::State;

use crate::auth::{oauth, token_store};
use crate::db::keys;
use crate::error::{AppError, AppResult};
use crate::models::{ConnectionStatus, TokenInfo};
use crate::state::{
    AppState, MAX_COMMENT_FETCH_LIMIT, MAX_LOOKBACK_HOURS, MAX_MEDIA_FETCH_LIMIT,
    MAX_POLLING_INTERVAL_SECS, MIN_COMMENT_FETCH_LIMIT, MIN_LOOKBACK_HOURS,
    MIN_MEDIA_FETCH_LIMIT, MIN_POLLING_INTERVAL_SECS,
};

/// App Dashboardで発行される長期トークンの有効期間 (60日)。
/// リフレッシュで実期限に同期できなかった場合のフォールバックとして使う
const LONG_LIVED_TOKEN_TTL_SECS: i64 = 60 * 24 * 60 * 60;

/// Instagramコメントの文字数上限。超える返信文は全コメントが恒久failedに
/// なり続けるため、保存時に弾く
const REPLY_TEXT_MAX_CHARS: usize = 2200;

#[derive(Debug, Serialize)]
pub struct StatusPayload {
    pub status: ConnectionStatus,
    pub last_run_at: Option<String>,
    pub sending_paused: bool,
    pub dry_run: bool,
    /// ポーリング実行中の進捗表示。周期の外ではnull
    pub progress: Option<String>,
    /// 一時エラーが連続していて自動再試行中 (UIに警告を出す)
    pub connection_issue: bool,
    /// 直近の周期でコメント取得が上限で打ち切られた (確認漏れの可能性を通知)
    pub fetch_truncated: bool,
    /// 直近の成功した周期の集計 (例: "リール3件・コメント12件を確認 / 返信2件")
    pub last_cycle_summary: Option<String>,
}

fn status_payload(state: &AppState) -> StatusPayload {
    StatusPayload {
        status: state.status(),
        last_run_at: state.last_run_at(),
        sending_paused: state.sending_paused(),
        dry_run: state.dry_run(),
        progress: state.cycle_progress(),
        connection_issue: state.connection_issue(),
        fetch_truncated: state.fetch_truncated(),
        last_cycle_summary: state.last_cycle_summary(),
    }
}

#[derive(Debug, Serialize)]
pub struct SettingsPayload {
    pub reply_text: String,
    pub polling_interval_secs: u64,
    pub media_fetch_limit: u32,
    pub comment_lookback_hours: i64,
    pub comment_fetch_limit: u32,
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> StatusPayload {
    status_payload(&state)
}

/// 初回起動時の利用条件に同意済みか
#[tauri::command]
pub fn get_terms_accepted(state: State<'_, AppState>) -> bool {
    state.terms_accepted()
}

/// 利用条件への同意を記録する (同意日時を保存。再実行しても上書きされるだけで無害)
#[tauri::command]
pub fn accept_terms(state: State<'_, AppState>) -> AppResult<()> {
    state.set_terms_accepted(&Utc::now().to_rfc3339())?;
    log::info!("利用条件への同意を記録しました");
    Ok(())
}

/// ドライランの切り替え。解除すると実際の送信が始まる
#[tauri::command]
pub fn set_dry_run(state: State<'_, AppState>, enabled: bool) -> AppResult<StatusPayload> {
    state.set_dry_run(enabled)?;
    log::warn!(
        "ドライランを{}にしました",
        if enabled { "有効 (送信なし)" } else { "無効 (実送信開始)" }
    );
    Ok(status_payload(&state))
}

/// kill switch: 新規送信の一時停止/再開
#[tauri::command]
pub fn set_sending_paused(state: State<'_, AppState>, paused: bool) -> AppResult<StatusPayload> {
    state.set_sending_paused(paused)?;
    log::warn!(
        "kill switchを{}にしました",
        if paused { "有効 (送信停止)" } else { "無効 (送信再開)" }
    );
    Ok(status_payload(&state))
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> SettingsPayload {
    SettingsPayload {
        reply_text: state.reply_text(),
        polling_interval_secs: state.polling_interval_secs(),
        media_fetch_limit: state.media_fetch_limit(),
        comment_lookback_hours: state.comment_lookback_hours(),
        comment_fetch_limit: state.comment_fetch_limit(),
    }
}

/// 取得範囲の設定: 対象リール数・コメント対象期間・1リールあたりのコメント確認上限
#[tauri::command]
pub fn save_fetch_settings(
    state: State<'_, AppState>,
    media_fetch_limit: u32,
    comment_lookback_hours: i64,
    comment_fetch_limit: u32,
) -> AppResult<()> {
    if !(MIN_MEDIA_FETCH_LIMIT..=MAX_MEDIA_FETCH_LIMIT).contains(&media_fetch_limit) {
        return Err(AppError::Config(format!(
            "対象リール数は{}〜{}件で指定してください",
            MIN_MEDIA_FETCH_LIMIT, MAX_MEDIA_FETCH_LIMIT
        )));
    }
    if !(MIN_COMMENT_FETCH_LIMIT..=MAX_COMMENT_FETCH_LIMIT).contains(&comment_fetch_limit) {
        return Err(AppError::Config(format!(
            "コメント確認上限は{}〜{}件で指定してください",
            MIN_COMMENT_FETCH_LIMIT, MAX_COMMENT_FETCH_LIMIT
        )));
    }
    if !(MIN_LOOKBACK_HOURS..=MAX_LOOKBACK_HOURS).contains(&comment_lookback_hours) {
        return Err(AppError::Config(format!(
            "コメント対象期間は{}〜{}時間で指定してください",
            MIN_LOOKBACK_HOURS, MAX_LOOKBACK_HOURS
        )));
    }
    // 対象期間 < ポーリング間隔 だとチェックの合間に対象外へ抜けるコメントが出て
    // 取りこぼすため、組み合わせとして拒否する
    if (comment_lookback_hours as u64) * 3600 < state.polling_interval_secs() {
        return Err(AppError::Config(
            "コメント対象期間はポーリング間隔より長くしてください (取りこぼし防止)".into(),
        ));
    }
    state
        .db
        .set_setting(keys::MEDIA_FETCH_LIMIT, &media_fetch_limit.to_string())?;
    state.db.set_setting(
        keys::COMMENT_LOOKBACK_HOURS,
        &comment_lookback_hours.to_string(),
    )?;
    state
        .db
        .set_setting(keys::COMMENT_FETCH_LIMIT, &comment_fetch_limit.to_string())?;
    log::info!(
        "取得範囲を保存: リール{}件 / 過去{}時間 / コメント上限{}件",
        media_fetch_limit,
        comment_lookback_hours,
        comment_fetch_limit
    );
    Ok(())
}

#[tauri::command]
pub fn save_reply_text(state: State<'_, AppState>, text: String) -> AppResult<()> {
    if text.trim().is_empty() {
        return Err(AppError::Config("返信文を入力してください".into()));
    }
    if text.chars().count() > REPLY_TEXT_MAX_CHARS {
        return Err(AppError::Config(format!(
            "返信文は{}文字以内で入力してください",
            REPLY_TEXT_MAX_CHARS
        )));
    }
    state.db.set_setting(keys::REPLY_TEXT, &text)
}

#[tauri::command]
pub fn save_polling_interval(state: State<'_, AppState>, secs: u64) -> AppResult<()> {
    if !(MIN_POLLING_INTERVAL_SECS..=MAX_POLLING_INTERVAL_SECS).contains(&secs) {
        return Err(AppError::Config(format!(
            "ポーリング間隔は{}〜{}秒で指定してください",
            MIN_POLLING_INTERVAL_SECS, MAX_POLLING_INTERVAL_SECS
        )));
    }
    // ポーリング間隔 > コメント対象期間 だと取りこぼすため、組み合わせとして拒否する
    if secs > (state.comment_lookback_hours() as u64) * 3600 {
        return Err(AppError::Config(
            "ポーリング間隔はコメント対象期間より短くしてください (取りこぼし防止)".into(),
        ));
    }
    state
        .db
        .set_setting(keys::POLLING_INTERVAL_SECS, &secs.to_string())
}

/// 貼り付けられた長期アクセストークンを検証し、保存して接続状態にする
#[tauri::command]
pub async fn connect_with_token(
    state: State<'_, AppState>,
    token: String,
) -> Result<StatusPayload, AppError> {
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err(AppError::Auth("アクセストークンを入力してください".into()));
    }

    let user_id = state.ig.get_me(&token).await.map_err(|e| match e {
        // 無効なトークンの貼り付けは「期限切れ」ではなく入力誤りとして伝える
        AppError::TokenExpired => {
            AppError::Auth("トークンが無効です。発行し直して貼り付けてください".into())
        }
        other => other,
    })?;

    let provisional = TokenInfo {
        access_token: token,
        user_id,
        expires_at: Utc::now().timestamp() + LONG_LIVED_TOKEN_TTL_SECS,
    };

    // 貼り付けトークンの実期限はAPIから取得できないため、リフレッシュを一度試みて
    // 実期限に同期する (残り期間の短いトークンを60日有効と誤認すると
    // 期限7日前の自動リフレッシュが機能しない)。
    // 発行24時間未満のトークンはリフレッシュできないため、失敗時は60日仮定で続行する
    let http = crate::api::instagram::http_client();
    let token_info = match oauth::refresh_token(&http, &provisional).await {
        Ok(refreshed) => refreshed,
        Err(e) => {
            log::info!(
                "トークン期限の同期に失敗 (貼り付け時点から60日として扱う): {}",
                e
            );
            provisional
        }
    };

    token_store::save(&token_info)?;
    state.set_token(Some(token_info));
    state.set_status(ConnectionStatus::Connected);
    log::info!("アクセストークンを検証してInstagramアカウントを連携しました");

    Ok(status_payload(&state))
}
