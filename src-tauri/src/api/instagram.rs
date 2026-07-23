use std::sync::RwLock;
use std::time::Duration;

use reqwest::{Response, StatusCode};
use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::models::{Comment, Media};

/// アクセストークン期限切れを表すOAuthエラーコード
const ERROR_CODE_OAUTH: i64 = 190;

/// HTTP 429以外でレート制限を表すMetaエラーコード
/// (4: アプリ制限, 17: ユーザー制限, 32: ページ制限, 613: カスタム制限)
const RATE_LIMIT_ERROR_CODES: [i64; 4] = [4, 17, 32, 613];

/// タイムアウトを設定したHTTPクライアントを生成する。
/// 全体タイムアウトがないとハング時にポーリングが無期限停止し、
/// 「タイムアウト→結果不明」の二重返信防止も機能しないため必須
pub fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .connect_timeout(Duration::from_secs(10))
        .build()
        // builderの失敗はTLS初期化不能など環境異常のみ。既定クライアントで継続する
        .unwrap_or_else(|_| reqwest::Client::new())
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    data: Vec<T>,
    paging: Option<Paging>,
}

#[derive(Debug, Deserialize)]
struct Paging {
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ApiErrorBody {
    error: ApiErrorDetail,
}

#[derive(Debug, Deserialize)]
struct ApiErrorDetail {
    message: Option<String>,
    code: Option<i64>,
    is_transient: Option<bool>,
    fbtrace_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ReplyResponse {
    id: String,
}

#[derive(Debug, Deserialize)]
struct MeResponse {
    /// Graph APIのパスで使うプロアカウントID (数値で返ることがある)
    user_id: Option<serde_json::Value>,
    id: Option<String>,
}

impl MeResponse {
    fn user_id(&self) -> Option<String> {
        match &self.user_id {
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(other) => Some(other.to_string()),
            None => self.id.clone(),
        }
    }
}

/// 使用量ヘッダーの1エントリ (使用率%)。
/// x-app-usage はこの形そのもの、x-business-use-case-usage は
/// {"<business-id>": [この形, ...]} で返る (未知のフィールドは無視)
#[derive(Debug, Deserialize)]
struct AppUsage {
    call_count: Option<f64>,
    total_time: Option<f64>,
    total_cputime: Option<f64>,
}

impl AppUsage {
    fn max_pct(&self) -> f64 {
        [self.call_count, self.total_time, self.total_cputime]
            .into_iter()
            .flatten()
            .fold(0.0, f64::max)
    }
}

pub struct InstagramClient {
    http: reqwest::Client,
    base: String,
    /// 直近のレスポンスで観測したAPI使用率 (%)
    last_usage_pct: RwLock<Option<f64>>,
}

impl InstagramClient {
    pub fn new(api_version: &str) -> Self {
        Self {
            http: http_client(),
            base: format!("https://graph.instagram.com/{}", api_version),
            last_usage_pct: RwLock::new(None),
        }
    }

    /// 直近に観測したAPI使用率 (%)。まだ観測していなければNone
    pub fn last_usage_pct(&self) -> Option<f64> {
        self.last_usage_pct.read().ok().and_then(|v| *v)
    }

    /// トークンの有効性を検証し、自分のユーザーIDを返す
    ///
    /// トークンはURLに残さないようAuthorizationヘッダーで送る (ログへの漏えい防止)
    pub async fn get_me(&self, access_token: &str) -> AppResult<String> {
        let response = self
            .http
            .get(format!("{}/me", self.base))
            .query(&[("fields", "user_id")])
            .bearer_auth(access_token)
            .send()
            .await?;
        let me: MeResponse = self.parse_response(response).await?;
        me.user_id()
            .ok_or_else(|| AppError::Auth("ユーザーIDを取得できませんでした".into()))
    }

    /// 自分の最近のメディア一覧を取得する (リール以外も含む)
    pub async fn fetch_recent_media(
        &self,
        access_token: &str,
        limit: u32,
    ) -> AppResult<Vec<Media>> {
        let response = self
            .http
            .get(format!("{}/me/media", self.base))
            .query(&[
                ("fields", "id,media_product_type"),
                ("limit", &limit.to_string()),
            ])
            .bearer_auth(access_token)
            .send()
            .await?;
        let envelope: Envelope<Media> = self.parse_response(response).await?;
        Ok(envelope.data)
    }

    /// メディアに付いたトップレベルコメントを取得する
    pub async fn fetch_comments(
        &self,
        access_token: &str,
        media_id: &str,
    ) -> AppResult<Vec<Comment>> {
        let response = self
            .http
            .get(format!("{}/{}/comments", self.base, media_id))
            .query(&[("fields", "id,text,timestamp,from"), ("limit", "50")])
            .bearer_auth(access_token)
            .send()
            .await?;
        let envelope: Envelope<Comment> = self.parse_response(response).await?;
        if envelope.paging.and_then(|p| p.next).is_some() {
            // MVPでは1ページ (50件) のみ処理する。溢れた分は次回以降のポーリングで
            // 拾えないため、頻発するようならページネーション対応が必要
            log::warn!(
                "media_id={} のコメントが50件を超えています。超過分は処理されません",
                media_id
            );
        }
        Ok(envelope.data)
    }

    /// コメントに付いた返信一覧を取得する (再送前の二重返信チェック用)
    pub async fn fetch_replies(
        &self,
        access_token: &str,
        comment_id: &str,
    ) -> AppResult<Vec<Comment>> {
        let response = self
            .http
            .get(format!("{}/{}/replies", self.base, comment_id))
            .query(&[("fields", "id,from"), ("limit", "50")])
            .bearer_auth(access_token)
            .send()
            .await?;
        let envelope: Envelope<Comment> = self.parse_response(response).await?;
        Ok(envelope.data)
    }

    /// コメントへ返信し、作成された返信コメントのIDを返す
    pub async fn reply_to_comment(
        &self,
        access_token: &str,
        comment_id: &str,
        message: &str,
    ) -> AppResult<String> {
        let response = self
            .http
            .post(format!("{}/{}/replies", self.base, comment_id))
            .form(&[("message", message)])
            .bearer_auth(access_token)
            .send()
            .await?;
        let reply: ReplyResponse = self.parse_response(response).await?;
        Ok(reply.id)
    }

    async fn parse_response<T: DeserializeOwned>(&self, response: Response) -> AppResult<T> {
        self.record_usage(&response);
        let status = response.status();
        let retry_after = parse_retry_after(&response);
        let body = response.text().await?;

        if status == StatusCode::TOO_MANY_REQUESTS {
            return Err(AppError::RateLimited {
                retry_after_secs: retry_after,
            });
        }
        if !status.is_success() {
            return Err(to_api_error(status, &body, retry_after));
        }
        serde_json::from_str(&body)
            .map_err(|e| AppError::Other(format!("APIレスポンスのパースに失敗しました: {}", e)))
    }

    fn record_usage(&self, response: &Response) {
        let header = |name: &str| {
            response
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
        };
        let Some(pct) = max_usage_pct(header("x-app-usage"), header("x-business-use-case-usage"))
        else {
            return;
        };
        if let Ok(mut guard) = self.last_usage_pct.write() {
            *guard = Some(pct);
        }
    }
}

/// アプリ単位 (x-app-usage) とアカウント単位 (x-business-use-case-usage) の
/// 両ヘッダーから使用率の最大値を求める。アカウント別レート上限は
/// business use case側でしか報告されないため、片方だけでは取りこぼす
fn max_usage_pct(app_header: Option<&str>, business_header: Option<&str>) -> Option<f64> {
    let app_pct = app_header
        .and_then(|v| serde_json::from_str::<AppUsage>(v).ok())
        .map(|u| u.max_pct());
    let business_pct = business_header
        .and_then(|v| {
            serde_json::from_str::<std::collections::HashMap<String, Vec<AppUsage>>>(v).ok()
        })
        .map(|m| {
            m.values()
                .flatten()
                .fold(0.0, |acc, u| f64::max(acc, u.max_pct()))
        });
    match (app_pct, business_pct) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    }
}

fn parse_retry_after(response: &Response) -> Option<u64> {
    response
        .headers()
        .get(reqwest::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
}

fn to_api_error(status: StatusCode, body: &str, retry_after: Option<u64>) -> AppError {
    match serde_json::from_str::<ApiErrorBody>(body) {
        Ok(parsed) => {
            let code = parsed.error.code.unwrap_or(-1);
            if code == ERROR_CODE_OAUTH {
                return AppError::TokenExpired;
            }
            // Metaはレート制限をHTTP 400 + エラーコードで返すことがある
            if RATE_LIMIT_ERROR_CODES.contains(&code) {
                return AppError::RateLimited {
                    retry_after_secs: retry_after,
                };
            }
            AppError::Api {
                code,
                http_status: status.as_u16(),
                message: parsed
                    .error
                    .message
                    .unwrap_or_else(|| "unknown error".to_string()),
                fbtrace_id: parsed.error.fbtrace_id,
                is_transient: parsed.error.is_transient.unwrap_or(false),
            }
        }
        Err(_) => AppError::Api {
            code: -1,
            http_status: status.as_u16(),
            message: format!("HTTP {}", status),
            fbtrace_id: None,
            is_transient: false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_error_code_maps_to_token_expired() {
        let body = r#"{"error":{"message":"Invalid OAuth access token","code":190}}"#;
        let error = to_api_error(StatusCode::BAD_REQUEST, body, None);
        assert!(matches!(error, AppError::TokenExpired));
    }

    #[test]
    fn meta_rate_limit_codes_map_to_rate_limited_even_on_http_400() {
        for code in [4, 17, 32, 613] {
            let body = format!(r#"{{"error":{{"message":"limit reached","code":{}}}}}"#, code);
            let error = to_api_error(StatusCode::BAD_REQUEST, &body, Some(120));
            assert!(
                matches!(
                    error,
                    AppError::RateLimited {
                        retry_after_secs: Some(120)
                    }
                ),
                "code={} はRateLimitedになるべき",
                code
            );
        }
    }

    #[test]
    fn api_error_captures_code_fbtrace_and_transience() {
        let body = r#"{"error":{"message":"temporary","code":2,"is_transient":true,"fbtrace_id":"trace123"}}"#;
        let error = to_api_error(StatusCode::INTERNAL_SERVER_ERROR, body, None);
        match error {
            AppError::Api {
                code,
                http_status,
                message,
                fbtrace_id,
                is_transient,
            } => {
                assert_eq!(code, 2);
                assert_eq!(http_status, 500);
                assert_eq!(message, "temporary");
                assert_eq!(fbtrace_id.as_deref(), Some("trace123"));
                assert!(is_transient);
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn permanent_error_defaults_to_not_transient() {
        let body = r#"{"error":{"message":"Unsupported request","code":100}}"#;
        let error = to_api_error(StatusCode::BAD_REQUEST, body, None);
        match error {
            AppError::Api { is_transient, .. } => assert!(!is_transient),
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn missing_error_fields_fall_back_to_defaults() {
        let body = r#"{"error":{}}"#;
        let error = to_api_error(StatusCode::BAD_REQUEST, body, None);
        match error {
            AppError::Api { code, message, .. } => {
                assert_eq!(code, -1);
                assert_eq!(message, "unknown error");
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn unparsable_body_maps_to_api_error_with_status() {
        let error = to_api_error(StatusCode::INTERNAL_SERVER_ERROR, "<html>", None);
        match error {
            AppError::Api {
                code,
                http_status,
                message,
                ..
            } => {
                assert_eq!(code, -1);
                assert_eq!(http_status, 500);
                assert!(message.contains("500"));
            }
            other => panic!("unexpected error: {:?}", other),
        }
    }

    #[test]
    fn app_usage_takes_max_of_metrics() {
        let usage: AppUsage =
            serde_json::from_str(r#"{"call_count":10,"total_time":85,"total_cputime":3}"#).unwrap();
        assert_eq!(usage.max_pct(), 85.0);
    }

    #[test]
    fn app_usage_defaults_to_zero_when_empty() {
        let usage: AppUsage = serde_json::from_str(r#"{}"#).unwrap();
        assert_eq!(usage.max_pct(), 0.0);
    }

    #[test]
    fn max_usage_pct_takes_max_across_both_headers() {
        let app = r#"{"call_count":10,"total_time":20,"total_cputime":5}"#;
        // 未知フィールド (type, estimated_time_to_regain_access) は無視される
        let buc = r#"{"17841400000000000":[{"type":"instagram","call_count":95,"total_cputime":3,"total_time":8,"estimated_time_to_regain_access":0}]}"#;
        assert_eq!(max_usage_pct(Some(app), Some(buc)), Some(95.0));
        assert_eq!(max_usage_pct(Some(app), None), Some(20.0));
        assert_eq!(max_usage_pct(None, Some(buc)), Some(95.0));
        assert_eq!(max_usage_pct(None, None), None);
    }

    #[test]
    fn max_usage_pct_ignores_unparsable_headers() {
        assert_eq!(max_usage_pct(Some("<html>"), None), None);
        assert_eq!(
            max_usage_pct(Some(r#"{"call_count":50}"#), Some("broken")),
            Some(50.0)
        );
    }

    #[test]
    fn client_base_url_uses_configured_version() {
        let client = InstagramClient::new("v26.0");
        assert_eq!(client.base, "https://graph.instagram.com/v26.0");
    }

    #[test]
    fn envelope_deserializes_media_list() {
        let body = r#"{
            "data": [
                {"id": "1", "media_product_type": "REELS"},
                {"id": "2", "media_product_type": "FEED"}
            ],
            "paging": {"next": "https://example.com/next"}
        }"#;
        let envelope: Envelope<Media> = serde_json::from_str(body).unwrap();
        assert_eq!(envelope.data.len(), 2);
        assert!(envelope.data[0].is_reel());
        assert!(envelope.paging.and_then(|p| p.next).is_some());
    }

    #[test]
    fn me_response_normalizes_numeric_user_id() {
        let me: MeResponse = serde_json::from_str(r#"{"user_id": 17841400000000000}"#).unwrap();
        assert_eq!(me.user_id().unwrap(), "17841400000000000");
    }

    #[test]
    fn me_response_prefers_user_id_over_id() {
        let me: MeResponse = serde_json::from_str(r#"{"user_id": "123", "id": "999"}"#).unwrap();
        assert_eq!(me.user_id().unwrap(), "123");
    }

    #[test]
    fn me_response_falls_back_to_id_field() {
        let me: MeResponse = serde_json::from_str(r#"{"id": "999"}"#).unwrap();
        assert_eq!(me.user_id().unwrap(), "999");
    }

    #[test]
    fn me_response_without_ids_returns_none() {
        let me: MeResponse = serde_json::from_str(r#"{}"#).unwrap();
        assert!(me.user_id().is_none());
    }

    #[test]
    fn envelope_deserializes_comments_without_optional_fields() {
        let body = r#"{"data": [{"id": "c1"}]}"#;
        let envelope: Envelope<Comment> = serde_json::from_str(body).unwrap();
        assert_eq!(envelope.data[0].id, "c1");
        assert!(envelope.data[0].from.is_none());
        assert!(envelope.data[0].timestamp.is_none());
        assert!(envelope.data[0].text.is_none());
    }
}
