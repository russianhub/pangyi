use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::error::AccountError;

pub const EXPIRY_SKEW: Duration = Duration::from_secs(60);

#[derive(Clone)]
pub struct Session {
    pub email: String,
    pub user_id: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at_unix: u64,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("email", &self.email)
            .field("user_id", &self.user_id)
            .field("access_token", &"<redacted>")
            .field("refresh_token", &"<redacted>")
            .field("expires_at_unix", &self.expires_at_unix)
            .finish()
    }
}

impl Session {
    pub fn access_usable(&self, now: SystemTime) -> bool {
        let exp = UNIX_EPOCH + Duration::from_secs(self.expires_at_unix);
        match exp.duration_since(now) {
            Ok(left) => left > EXPIRY_SKEW,
            Err(_) => false,
        }
    }
}

pub fn encode_session(session: &Session) -> Result<Vec<u8>, AccountError> {
    for field in [
        session.email.as_str(),
        session.user_id.as_str(),
        session.access_token.as_str(),
        session.refresh_token.as_str(),
    ] {
        if field.is_empty() || field.bytes().any(|b| b == b'\n' || b == b'\r') {
            return Err(AccountError::Incomplete);
        }
    }
    if session.expires_at_unix == 0 {
        return Err(AccountError::Incomplete);
    }
    Ok(format!(
        "pangyi-session-v1\n{}\n{}\n{}\n{}\n{}\n",
        session.email,
        session.user_id,
        session.expires_at_unix,
        session.access_token,
        session.refresh_token
    )
    .into_bytes())
}

pub fn decode_session(bytes: &[u8]) -> Result<Session, AccountError> {
    let text = std::str::from_utf8(bytes).map_err(|_| AccountError::Incomplete)?;
    let mut lines = text.lines();
    if lines.next() != Some("pangyi-session-v1") {
        return Err(AccountError::Incomplete);
    }
    let session = Session {
        email: lines.next().unwrap_or("").to_string(),
        user_id: lines.next().unwrap_or("").to_string(),
        expires_at_unix: lines
            .next()
            .unwrap_or("")
            .parse()
            .map_err(|_| AccountError::Incomplete)?,
        access_token: lines.next().unwrap_or("").to_string(),
        refresh_token: lines.next().unwrap_or("").to_string(),
    };
    if lines.next().is_some() {
        return Err(AccountError::Incomplete);
    }
    encode_session(&session)?;
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_hides_tokens_in_debug_and_rejects_newlines() {
        let session = Session {
            email: "user@example.com".into(),
            user_id: "user-1".into(),
            access_token: "access-token-value".into(),
            refresh_token: "refresh-token-value".into(),
            expires_at_unix: 1_900_000_000,
        };
        let back = decode_session(&encode_session(&session).unwrap()).unwrap();
        assert_eq!(back.refresh_token, session.refresh_token);
        let shown = format!("{session:?}");
        assert!(!shown.contains("access-token-value"));
        assert!(!shown.contains("refresh-token-value"));
        let mut bad = session.clone();
        bad.access_token = "a\nb".into();
        assert!(encode_session(&bad).is_err());
        let now = UNIX_EPOCH + Duration::from_secs(1_900_000_000 - 30);
        assert!(!session.access_usable(now));
    }
}
