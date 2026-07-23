use keyring::Entry;

use crate::error::{AppError, AppResult};
use crate::models::TokenInfo;

const SERVICE: &str = "com.kenshinmorioka.instagram-crm";
const ACCOUNT: &str = "instagram-token";

fn entry() -> AppResult<Entry> {
    Entry::new(SERVICE, ACCOUNT).map_err(AppError::from)
}

pub fn save(token: &TokenInfo) -> AppResult<()> {
    let raw = serde_json::to_string(token)
        .map_err(|e| AppError::Other(format!("トークンのシリアライズに失敗: {}", e)))?;
    entry()?.set_password(&raw)?;
    Ok(())
}

pub fn load() -> AppResult<Option<TokenInfo>> {
    match entry()?.get_password() {
        Ok(raw) => {
            let token = serde_json::from_str(&raw)
                .map_err(|e| AppError::Other(format!("保存済みトークンのパースに失敗: {}", e)))?;
            Ok(Some(token))
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(AppError::from(e)),
    }
}
