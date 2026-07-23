use chrono::{DateTime, FixedOffset, Utc};
use serde::{Deserialize, Serialize};

/// Keychain / Credential Manager に保存するアクセストークン情報
#[derive(Clone, Serialize, Deserialize)]
pub struct TokenInfo {
    pub access_token: String,
    pub user_id: String,
    /// 有効期限 (Unixタイムスタンプ秒)
    pub expires_at: i64,
}

/// {:?} ログ経由でトークン実値が平文出力されないよう、deriveせず手動でマスクする
impl std::fmt::Debug for TokenInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenInfo")
            .field("access_token", &"***")
            .field("user_id", &self.user_id)
            .field("expires_at", &self.expires_at)
            .finish()
    }
}

impl TokenInfo {
    pub fn is_expired(&self) -> bool {
        self.expires_at <= Utc::now().timestamp()
    }

    pub fn expires_within_secs(&self, secs: i64) -> bool {
        self.expires_at - Utc::now().timestamp() <= secs
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Media {
    pub id: String,
    pub media_product_type: Option<String>,
}

impl Media {
    pub fn is_reel(&self) -> bool {
        self.media_product_type.as_deref() == Some("REELS")
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct CommentFrom {
    pub id: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Comment {
    pub id: String,
    pub text: Option<String>,
    pub timestamp: Option<String>,
    pub from: Option<CommentFrom>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    NotConnected,
    Connected,
    NeedsReauth,
}

/// Instagram APIのtimestamp ("2026-07-23T12:34:56+0000") をパースする
pub fn parse_ig_timestamp(value: &str) -> Option<DateTime<FixedOffset>> {
    DateTime::parse_from_str(value, "%Y-%m-%dT%H:%M:%S%z").ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_expiring_in(secs: i64) -> TokenInfo {
        TokenInfo {
            access_token: "t".into(),
            user_id: "u".into(),
            expires_at: Utc::now().timestamp() + secs,
        }
    }

    #[test]
    fn is_expired_is_false_before_expiry() {
        assert!(!token_expiring_in(60).is_expired());
    }

    #[test]
    fn is_expired_is_true_after_expiry() {
        assert!(token_expiring_in(-60).is_expired());
    }

    #[test]
    fn is_expired_is_true_at_exact_expiry() {
        assert!(token_expiring_in(0).is_expired());
    }

    #[test]
    fn expires_within_secs_boundary() {
        // 境界ちょうどはtrue、境界+余裕はfalse
        assert!(token_expiring_in(100).expires_within_secs(100));
        assert!(!token_expiring_in(200).expires_within_secs(100));
    }

    #[test]
    fn token_info_debug_masks_access_token() {
        let token = TokenInfo {
            access_token: "secret-token-value".into(),
            user_id: "u1".into(),
            expires_at: 0,
        };
        let debug = format!("{:?}", token);
        assert!(!debug.contains("secret-token-value"));
        assert!(debug.contains("***"));
        assert!(debug.contains("u1"));
    }

    #[test]
    fn is_reel_only_for_reels_product_type() {
        let reel = Media {
            id: "1".into(),
            media_product_type: Some("REELS".into()),
        };
        let feed = Media {
            id: "2".into(),
            media_product_type: Some("FEED".into()),
        };
        let unknown = Media {
            id: "3".into(),
            media_product_type: None,
        };
        assert!(reel.is_reel());
        assert!(!feed.is_reel());
        assert!(!unknown.is_reel());
    }

    #[test]
    fn parse_ig_timestamp_accepts_instagram_format() {
        let parsed = parse_ig_timestamp("2026-07-23T12:34:56+0000").unwrap();
        assert_eq!(parsed.timestamp(), 1784810096);
    }

    #[test]
    fn parse_ig_timestamp_rejects_invalid_values() {
        assert!(parse_ig_timestamp("").is_none());
        assert!(parse_ig_timestamp("2026-07-23").is_none());
        assert!(parse_ig_timestamp("not a date").is_none());
    }
}
