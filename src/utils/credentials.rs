use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use unicode_normalization::UnicodeNormalization;

#[derive(Debug, Deserialize, Serialize)]
struct OAuthCredentials {
    #[serde(rename = "accessToken")]
    access_token: String,
    #[serde(rename = "refreshToken")]
    refresh_token: Option<String>,
    #[serde(rename = "expiresAt")]
    expires_at: Option<u64>,
    scopes: Option<Vec<String>>,
    #[serde(rename = "subscriptionType")]
    subscription_type: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
struct CredentialsFile {
    #[serde(rename = "claudeAiOauth")]
    claude_ai_oauth: Option<OAuthCredentials>,
}

/// One Claude login. `dir == None` is the default `~/.claude` store.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CredentialStore {
    dir: Option<String>,
    service: String,
}

pub fn get_oauth_token() -> Option<String> {
    let store = credential_store_from_env();
    if cfg!(target_os = "macos") {
        if let Some(token) = read_keychain(&store.service) {
            return Some(token);
        }
    }
    read_store_file(&store)
}

/// Directory whose `ccline/.api_usage_cache.json` belongs to this login.
/// A second account must not reuse the default cache.
pub fn usage_cache_path() -> Option<PathBuf> {
    let base = match credential_store_from_env().dir {
        Some(dir) => PathBuf::from(dir),
        None => home_claude_dir()?,
    };
    Some(base.join("ccline").join(".api_usage_cache.json"))
}

/// `settings.json` for the Claude profile that launched this process.
/// Proxy settings live here, separate from the credential store.
pub fn claude_settings_path() -> Option<PathBuf> {
    let base = match std::env::var("CLAUDE_CONFIG_DIR") {
        Ok(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => home_claude_dir()?,
    };
    Some(base.join("settings.json"))
}

fn credential_store_from_env() -> CredentialStore {
    resolve_store(
        env_if_set("CLAUDE_SECURESTORAGE_CONFIG_DIR").as_deref(),
        env_if_set("CLAUDE_CONFIG_DIR").as_deref(),
        env_if_set("CLAUDE_CODE_CUSTOM_OAUTH_URL").is_some_and(|value| !value.is_empty()),
    )
}

/// `None` when the variable is unset. `Some("")` when it is set empty.
fn env_if_set(name: &str) -> Option<String> {
    std::env::var(name).ok()
}

/// Match Claude Code's credential store selection.
///
/// `CLAUDE_SECURESTORAGE_CONFIG_DIR` wins when it is set. An empty value pins
/// the default store even if `CLAUDE_CONFIG_DIR` is also set. Otherwise a
/// non-empty `CLAUDE_CONFIG_DIR` selects that directory. The keychain service
/// hashes the raw directory string (NFC only), so `~`, a trailing slash, and
/// an absolute path are different stores.
///
/// A local Claude Code build inserts `-local-oauth` into the service name.
/// This lookup does not detect that build. `CLAUDE_CODE_CUSTOM_OAUTH_URL`
/// inserts `-custom-oauth`, which is the case Claude Code documents.
fn resolve_store(
    secure_storage: Option<&str>,
    config_dir: Option<&str>,
    custom_oauth: bool,
) -> CredentialStore {
    let dir = if let Some(secure) = secure_storage {
        if secure.is_empty() {
            None
        } else {
            Some(secure.to_string())
        }
    } else {
        match config_dir {
            Some(dir) if !dir.is_empty() => Some(dir.to_string()),
            _ => None,
        }
    };

    let infix = if custom_oauth { "-custom-oauth" } else { "" };
    let mut service = format!("Claude Code{infix}-credentials");
    if let Some(dir) = &dir {
        service.push('-');
        service.push_str(&dir_key_suffix(dir));
    }

    CredentialStore { dir, service }
}

/// First 8 hex chars of SHA-256 over the NFC form of `dir`.
fn dir_key_suffix(dir: &str) -> String {
    let nfc: String = dir.nfc().collect();
    let digest = Sha256::digest(nfc.as_bytes());
    digest
        .iter()
        .take(4)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn keychain_account_label(user: &str) -> String {
    let valid = !user.is_empty()
        && user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if valid {
        user.to_string()
    } else {
        "claude-code-user".to_string()
    }
}

fn read_keychain(service: &str) -> Option<String> {
    use std::process::Command;

    let account = keychain_account_label(&std::env::var("USER").unwrap_or_default());
    let output = Command::new("security")
        .args(["find-generic-password", "-a", &account, "-w", "-s", service])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let json_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    token_from_json(&json_str)
}

fn read_store_file(store: &CredentialStore) -> Option<String> {
    let path = credentials_file_path(store)?;
    read_token_from_path(&path)
}

fn credentials_file_path(store: &CredentialStore) -> Option<PathBuf> {
    match &store.dir {
        Some(dir) => Some(PathBuf::from(dir).join(".credentials.json")),
        None => Some(home_claude_dir()?.join(".credentials.json")),
    }
}

fn home_claude_dir() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".claude"))
}

fn read_token_from_path(path: &Path) -> Option<String> {
    if !path.exists() {
        return None;
    }
    let content = std::fs::read_to_string(path).ok()?;
    token_from_json(&content)
}

fn token_from_json(json: &str) -> Option<String> {
    let creds_file: CredentialsFile = serde_json::from_str(json).ok()?;
    creds_file.claude_ai_oauth.map(|oauth| oauth.access_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn talcum_path_matches_known_keychain_suffix() {
        assert_eq!(dir_key_suffix("/Users/cdcd/.claude-talcum"), "635b1af0");
    }

    #[test]
    fn nfc_and_nfd_paths_share_a_suffix() {
        let composed = "/tmp/caf\u{00e9}";
        let decomposed = "/tmp/cafe\u{0301}";
        assert_eq!(dir_key_suffix(composed), dir_key_suffix(decomposed));
    }

    #[test]
    fn default_store_has_the_unsuffixed_service() {
        let store = resolve_store(None, None, false);
        assert_eq!(store.dir, None);
        assert_eq!(store.service, "Claude Code-credentials");
    }

    #[test]
    fn config_dir_selects_its_own_service() {
        let store = resolve_store(None, Some("/Users/cdcd/.claude-talcum"), false);
        assert_eq!(store.dir.as_deref(), Some("/Users/cdcd/.claude-talcum"));
        assert_eq!(store.service, "Claude Code-credentials-635b1af0");
    }

    #[test]
    fn empty_secure_storage_pins_the_default_store() {
        let store = resolve_store(Some(""), Some("/Users/cdcd/.claude-talcum"), false);
        assert_eq!(store.dir, None);
        assert_eq!(store.service, "Claude Code-credentials");
    }

    #[test]
    fn secure_storage_wins_over_config_dir() {
        let store = resolve_store(
            Some("/tmp/secure"),
            Some("/Users/cdcd/.claude-talcum"),
            false,
        );
        assert_eq!(store.dir.as_deref(), Some("/tmp/secure"));
        assert_eq!(
            store.service,
            format!("Claude Code-credentials-{}", dir_key_suffix("/tmp/secure"))
        );
    }

    #[test]
    fn custom_oauth_inserts_the_infix() {
        let store = resolve_store(None, Some("/Users/cdcd/.claude-talcum"), true);
        assert_eq!(
            store.service,
            "Claude Code-custom-oauth-credentials-635b1af0"
        );
    }

    #[test]
    fn empty_config_dir_stays_on_the_default_store() {
        let store = resolve_store(None, Some(""), false);
        assert_eq!(store.service, "Claude Code-credentials");
    }

    #[test]
    fn keychain_account_falls_back_when_the_user_is_not_a_label() {
        assert_eq!(keychain_account_label("cdcd"), "cdcd");
        assert_eq!(keychain_account_label("a.b_c-d"), "a.b_c-d");
        assert_eq!(keychain_account_label(""), "claude-code-user");
        assert_eq!(keychain_account_label("bad user"), "claude-code-user");
    }
}
