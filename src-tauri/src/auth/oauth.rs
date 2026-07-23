use chrono::Utc;
use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::models::TokenInfo;

const GRAPH_BASE: &str = "https://graph.instagram.com";

#[derive(Debug, Deserialize)]
struct RefreshTokenResponse {
    access_token: String,
    expires_in: i64,
}

/// 長期トークンをリフレッシュする (発行から24時間以上経過したトークンのみ可)
///
/// リフレッシュにApp Secretは不要なため、アプリは認証情報を持たずにトークンを維持できる
pub async fn refresh_token(http: &reqwest::Client, token: &TokenInfo) -> AppResult<TokenInfo> {
    // トークンはURLに残さないようAuthorizationヘッダーで送る (ログへの漏えい防止)
    let response = http
        .get(format!("{}/refresh_access_token", GRAPH_BASE))
        .query(&[("grant_type", "ig_refresh_token")])
        .bearer_auth(&token.access_token)
        .send()
        .await?;

    let status = response.status();
    let body = response.text().await?;
    if !status.is_success() {
        // トークン実値をログへ出さないため、エラーメッセージのみ抜き出す
        let message = serde_json::from_str::<serde_json::Value>(&body)
            .ok()
            .and_then(|v| {
                v.get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_else(|| format!("HTTP {}", status));
        return Err(AppError::Auth(format!("トークンのリフレッシュに失敗: {}", message)));
    }

    let refreshed: RefreshTokenResponse = serde_json::from_str(&body)
        .map_err(|e| AppError::Auth(format!("トークンレスポンスのパースに失敗: {}", e)))?;
    Ok(TokenInfo {
        access_token: refreshed.access_token,
        user_id: token.user_id.clone(),
        expires_at: Utc::now().timestamp() + refreshed.expires_in,
    })
}
