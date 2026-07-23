use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::error::AppError;
use crate::models::ConnectionStatus;
use crate::services::auto_reply;
use crate::state::AppState;

/// 429時の指数バックオフの最大倍率 (30秒設定なら 30→60→120→240)
const MAX_BACKOFF_LEVEL: u32 = 3;

/// 一時エラーがこの回数連続したらUIに「接続に問題」警告を出す。
/// 単発の失敗は次周期の再試行で自動回復するため騒がない
const CONNECTION_ISSUE_THRESHOLD: u32 = 3;

/// バックグラウンドのポーリングループを起動する
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let http = crate::api::instagram::http_client();
        let mut backoff_level: u32 = 0;
        let mut retry_after: Option<u64> = None;
        let mut consecutive_failures: u32 = 0;

        loop {
            let state = app.state::<AppState>();
            let base_interval = state.polling_interval_secs();

            if state.status() == ConnectionStatus::Connected {
                // 周期実行中の再連携で有効になった新トークンを、旧トークンの失効エラーで
                // NeedsReauthに上書きしないよう、実行前のトークンを控えておく
                let token_before_cycle = state.token().map(|t| t.access_token);
                let cycle_result = auto_reply::run_cycle(&state, &http).await;
                // 早期return・エラーを含む全経路で進捗表示を確実に消す
                state.set_cycle_progress(None);
                match cycle_result {
                    Ok(report) => {
                        backoff_level = 0;
                        retry_after = None;
                        consecutive_failures = 0;
                        state.set_connection_issue(false);
                        if report.has_activity() {
                            log::info!(
                                "ポーリング完了: 取得={} 返信={} 失敗={} 不明={} 再試行待ち={} dry_run={}",
                                report.fetched_comments,
                                report.replied,
                                report.failed,
                                report.unknown,
                                report.requeued,
                                report.dry_run_matched
                            );
                        }
                    }
                    Err(AppError::RateLimited { retry_after_secs }) => {
                        backoff_level = (backoff_level + 1).min(MAX_BACKOFF_LEVEL);
                        retry_after = retry_after_secs;
                        // レート制限は接続自体は生きているため、接続警告の対象にしない
                        consecutive_failures = 0;
                        log::warn!(
                            "Rate Limitに到達。バックオフします (レベル{} Retry-After={:?})",
                            backoff_level,
                            retry_after_secs
                        );
                    }
                    Err(AppError::TokenExpired) => {
                        backoff_level = 0;
                        retry_after = None;
                        // 再連携要求はUIで別途表示されるため、接続警告の対象にしない
                        consecutive_failures = 0;
                        // 周期中にトークンが差し替わっていれば失効したのは旧トークン。
                        // 新トークンでの接続状態を維持し、次周期に判断を委ねる
                        if state.token().map(|t| t.access_token) != token_before_cycle {
                            log::warn!(
                                "周期実行中にトークンが再設定されたため、再連携要求をスキップします"
                            );
                        } else {
                            state.set_status(ConnectionStatus::NeedsReauth);
                            log::error!("トークン期限切れ: トークンの再設定が必要です");
                            show_main_window(&app);
                        }
                    }
                    Err(e) => {
                        // 一時的なAPIエラーは次回ポーリングで再試行する。
                        // 連続するようならUIに警告を出してユーザーが気付けるようにする
                        consecutive_failures += 1;
                        if consecutive_failures >= CONNECTION_ISSUE_THRESHOLD {
                            state.set_connection_issue(true);
                        }
                        log::error!("APIエラー ({}回連続): {}", consecutive_failures, e);
                    }
                }
            }

            let wait =
                compute_wait_secs(base_interval, backoff_level, retry_after, fastrand::f64());
            tokio::time::sleep(Duration::from_secs(wait)).await;
        }
    });
}

/// 次のポーリングまでの待機秒数を計算する。
/// バックオフ中は指数バックオフ + full jitter (下限は通常間隔)、
/// Retry-Afterが返っていればそれ以上待つ
fn compute_wait_secs(
    base_interval: u64,
    backoff_level: u32,
    retry_after: Option<u64>,
    jitter: f64,
) -> u64 {
    let wait = if backoff_level == 0 {
        base_interval
    } else {
        let cap = base_interval.saturating_mul(2u64.saturating_pow(backoff_level));
        ((cap as f64) * jitter).max(base_interval as f64) as u64
    };
    wait.max(retry_after.unwrap_or(0))
}

/// 再連携が必要になったらメイン画面を出して気付けるようにする
fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_cycle_waits_base_interval() {
        assert_eq!(compute_wait_secs(30, 0, None, 0.5), 30);
    }

    #[test]
    fn backoff_grows_exponentially_with_full_jitter() {
        // jitter=1.0で上限 (30*2^3=240)、jitter=0.0でも通常間隔を下回らない
        assert_eq!(compute_wait_secs(30, 3, None, 1.0), 240);
        assert_eq!(compute_wait_secs(30, 3, None, 0.0), 30);
        assert_eq!(compute_wait_secs(30, 1, None, 1.0), 60);
    }

    #[test]
    fn retry_after_is_respected_as_lower_bound() {
        assert_eq!(compute_wait_secs(30, 1, Some(300), 0.5), 300);
        // Retry-Afterがバックオフより短ければバックオフ側を使う
        assert_eq!(compute_wait_secs(30, 3, Some(10), 1.0), 240);
    }

    #[test]
    fn retry_after_applies_even_without_backoff() {
        assert_eq!(compute_wait_secs(30, 0, Some(90), 0.5), 90);
    }
}
