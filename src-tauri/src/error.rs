use serde::{Serialize, Serializer};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("HTTP通信エラー: {0}")]
    Http(#[from] reqwest::Error),

    #[error("DBエラー: {0}")]
    Db(#[from] rusqlite::Error),

    #[error("トークン保存領域エラー: {0}")]
    Keyring(#[from] keyring::Error),

    #[error("I/Oエラー: {0}")]
    Io(#[from] std::io::Error),

    #[error("設定エラー: {0}")]
    Config(String),

    #[error("認証エラー: {0}")]
    Auth(String),

    #[error("アクセストークンの有効期限が切れています。再連携してください")]
    TokenExpired,

    #[error("Rate Limitに到達しました (HTTP 429)")]
    RateLimited { retry_after_secs: Option<u64> },

    #[error("Instagram APIエラー (code={code}, http={http_status}): {message}")]
    Api {
        code: i64,
        http_status: u16,
        message: String,
        fbtrace_id: Option<String>,
        is_transient: bool,
    },

    #[error("{0}")]
    Other(String),
}

impl AppError {
    /// 一時的なエラーとして次周期での再試行が妥当か
    /// (権限不足・無効トークン・不正IDなどの恒久エラーは再試行しない)
    pub fn is_retryable_transient(&self) -> bool {
        match self {
            AppError::RateLimited { .. } => true,
            AppError::Api {
                is_transient,
                http_status,
                ..
            } => *is_transient || (500..=599).contains(&(*http_status as i64)),
            // 接続前に失敗したネットワークエラーは送信されていないため再試行可
            AppError::Http(e) => e.is_connect(),
            _ => false,
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api_error(http_status: u16, is_transient: bool) -> AppError {
        AppError::Api {
            code: 1,
            http_status,
            message: "test".into(),
            fbtrace_id: None,
            is_transient,
        }
    }

    #[test]
    fn rate_limited_is_retryable() {
        assert!(AppError::RateLimited {
            retry_after_secs: None
        }
        .is_retryable_transient());
    }

    #[test]
    fn transient_api_error_is_retryable() {
        assert!(api_error(400, true).is_retryable_transient());
    }

    #[test]
    fn server_errors_are_retryable() {
        assert!(api_error(500, false).is_retryable_transient());
        assert!(api_error(503, false).is_retryable_transient());
    }

    #[test]
    fn permanent_client_errors_are_not_retryable() {
        assert!(!api_error(400, false).is_retryable_transient());
        assert!(!api_error(403, false).is_retryable_transient());
    }

    #[test]
    fn auth_errors_are_not_retryable() {
        assert!(!AppError::TokenExpired.is_retryable_transient());
        assert!(!AppError::Auth("x".into()).is_retryable_transient());
    }
}
