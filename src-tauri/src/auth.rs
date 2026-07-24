use base64::Engine;
use rusqlite::Connection;
use serde::Deserialize;
use std::env;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct JwtClaims {
    sub: String,
}

pub fn resolve_session_cookie() -> Result<String, String> {
    if let Ok(raw) = env::var("CURSOR_SESSION_TOKEN") {
        let raw = raw.trim().to_string();
        if !raw.is_empty() {
            return Ok(normalize_cookie_value(&raw)?);
        }
    }

    let jwt = read_access_token_from_state_db()?;
    let sub = jwt_sub(&jwt)?;
    Ok(format!("{sub}::{jwt}"))
}

fn normalize_cookie_value(raw: &str) -> Result<String, String> {
    // Accept pasted WorkosCursorSessionToken (`sub::jwt` or URL-encoded).
    let decoded = raw.replace("%3A%3A", "::").replace("%3a%3a", "::");
    if decoded.contains("::") {
        return Ok(decoded);
    }
    let sub = jwt_sub(&decoded)?;
    Ok(format!("{sub}::{decoded}"))
}

fn state_db_path() -> Result<PathBuf, String> {
    let appdata = env::var("APPDATA").map_err(|_| "APPDATA is not set".to_string())?;
    Ok(PathBuf::from(appdata)
        .join("Cursor")
        .join("User")
        .join("globalStorage")
        .join("state.vscdb"))
}

fn read_access_token_from_state_db() -> Result<String, String> {
    let path = state_db_path()?;
    if !path.exists() {
        return Err(format!(
            "Cursor state DB not found at {}. Sign in to Cursor, or set CURSOR_SESSION_TOKEN.",
            path.display()
        ));
    }

    let conn = Connection::open(&path).map_err(|e| format!("open state.vscdb: {e}"))?;
    let token: String = conn
        .query_row(
            "SELECT value FROM ItemTable WHERE key = ?1",
            ["cursorAuth/accessToken"],
            |row| row.get(0),
        )
        .map_err(|e| format!("read cursorAuth/accessToken: {e}"))?;

    let token = token.trim().to_string();
    if token.is_empty() {
        return Err("cursorAuth/accessToken is empty — sign in to Cursor".into());
    }
    Ok(token)
}

fn jwt_sub(jwt: &str) -> Result<String, String> {
    let payload = jwt
        .split('.')
        .nth(1)
        .ok_or_else(|| "access token is not a JWT".to_string())?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .or_else(|_| base64::engine::general_purpose::URL_SAFE.decode(payload))
        .map_err(|e| format!("decode JWT payload: {e}"))?;
    let claims: JwtClaims =
        serde_json::from_slice(&bytes).map_err(|e| format!("parse JWT claims: {e}"))?;
    if claims.sub.is_empty() {
        return Err("JWT missing sub".into());
    }
    Ok(claims.sub)
}
