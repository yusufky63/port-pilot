//! Masks secrets in command lines before they are shown, printed or copied. Command lines are
//! read from other processes and regularly carry tokens (`--token=...`, `DATABASE_URL=...`,
//! `https://user:pass@host`). Redaction errs towards masking; `--no-redact` shows the raw text.

use crate::cmdline::raw_spans;

pub const MASK: &str = "****";

/// Key fragments that mark the value after them as secret.
const SECRET_KEYS: &[&str] = &[
    "token",
    "secret",
    "password",
    "passwd",
    "passphrase",
    "pwd",
    "apikey",
    "api_key",
    "api-key",
    "auth",
    "credential",
    "privatekey",
    "private_key",
    "private-key",
    "accesskey",
    "access_key",
    "access-key",
    "cookie",
    "session",
    "signature",
    "dsn",
    "database_url",
    "database-url",
    "connection_string",
    "connection-string",
    "connectionstring",
];

/// Prefixes of well-known token formats that are secret wherever they appear.
const TOKEN_PREFIXES: &[&str] = &[
    "ghp_",
    "gho_",
    "ghu_",
    "ghs_",
    "ghr_",
    "github_pat_",
    "glpat-",
    "sk-",
    "sk_live_",
    "sk_test_",
    "rk_live_",
    "pk_live_",
    "xoxb-",
    "xoxp-",
    "xoxa-",
    "AKIA",
    "ASIA",
    "AIza",
    "npm_",
    "hf_",
    "dop_v1_",
    "shpat_",
];

fn is_secret_key(key: &str) -> bool {
    let k = key
        .trim_start_matches(['-', '/'])
        .trim_matches('"')
        .to_ascii_lowercase();
    if k.is_empty() || k == "author" || k.starts_with("author") {
        return false;
    }
    k == "key" || k == "pass" || SECRET_KEYS.iter().any(|s| k.contains(s))
}

fn looks_like_token(value: &str) -> bool {
    let v = value.trim_matches('"');
    if v.len() >= 20 && TOKEN_PREFIXES.iter().any(|p| v.starts_with(p)) {
        return true;
    }
    // A JWT: three base64url segments, the first one a JSON object.
    if v.starts_with("eyJ") && v.len() > 30 && v.matches('.').count() == 2 {
        return true;
    }
    false
}

/// Replaces the password in `scheme://user:password@host` URLs inside `text`.
fn mask_url_userinfo(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find("://") {
        let (head, tail) = rest.split_at(pos + 3);
        out.push_str(head);
        let end = tail
            .find(|c: char| c == '/' || c == '?' || c == '#' || c.is_whitespace() || c == '"')
            .unwrap_or(tail.len());
        let authority = &tail[..end];
        match (authority.rfind('@'), authority.find(':')) {
            (Some(at), Some(colon)) if colon < at => {
                out.push_str(&authority[..=colon]);
                out.push_str(MASK);
                out.push_str(&authority[at..]);
            }
            _ => out.push_str(authority),
        }
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

fn quote_wrapped(token: &str) -> bool {
    token.len() >= 2 && token.starts_with('"') && token.ends_with('"')
}

fn mask_value(token: &str) -> String {
    if quote_wrapped(token) {
        format!("\"{MASK}\"")
    } else {
        MASK.to_string()
    }
}

/// Redacts one token; `prev_secret` says the previous token was a bare secret flag
/// (`--token abc`).
fn redact_token(token: &str, prev_secret: bool) -> (String, bool) {
    if prev_secret && !token.starts_with('-') {
        return (mask_value(token), false);
    }
    let inner = token.trim_matches('"');
    // --key=value, KEY=value, /key:value
    let split = inner
        .find('=')
        .or_else(|| inner.starts_with('/').then(|| inner.find(':')).flatten());
    if let Some(i) = split {
        let (key, value) = (&inner[..i], &inner[i + 1..]);
        if !value.is_empty() && (is_secret_key(key) || looks_like_token(value)) {
            let sep = &inner[i..=i];
            let masked = format!("{key}{sep}{MASK}");
            return (
                if quote_wrapped(token) {
                    format!("\"{masked}\"")
                } else {
                    masked
                },
                false,
            );
        }
        return (mask_url_userinfo(token), false);
    }
    if looks_like_token(inner) {
        return (mask_value(token), false);
    }
    let bare_flag = (inner.starts_with('-') || inner.starts_with('/')) && is_secret_key(inner);
    (mask_url_userinfo(token), bare_flag)
}

/// Returns `cmd` with secret values replaced by `****`, whitespace and quoting preserved.
pub fn redact_command_line(cmd: &str) -> String {
    let mut out = String::with_capacity(cmd.len());
    let mut last = 0;
    let mut prev_secret = false;
    for (a, b) in raw_spans(cmd) {
        out.push_str(&cmd[last..a]);
        let (masked, secret_flag) = redact_token(&cmd[a..b], prev_secret);
        out.push_str(&masked);
        prev_secret = secret_flag;
        last = b;
    }
    out.push_str(&cmd[last..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_ordinary_command_lines() {
        let cmd = r#""C:\Program Files\nodejs\node.exe" C:\src\shop\node_modules\vite\bin\vite.js --port 5173 --host"#;
        assert_eq!(redact_command_line(cmd), cmd);
        assert_eq!(
            redact_command_line("git commit --author=me"),
            "git commit --author=me"
        );
    }

    #[test]
    fn masks_key_value_flags() {
        assert_eq!(
            redact_command_line("app --token=abc123 --port 3000"),
            "app --token=**** --port 3000"
        );
        assert_eq!(
            redact_command_line("app --api-key abc123 --x"),
            "app --api-key **** --x"
        );
        assert_eq!(
            redact_command_line("app --password \"my secret\""),
            "app --password \"****\""
        );
        assert_eq!(
            redact_command_line("app /Password:hunter2"),
            "app /Password:****"
        );
        assert_eq!(
            redact_command_line("env GITHUB_TOKEN=abc node x.js"),
            "env GITHUB_TOKEN=**** node x.js"
        );
        assert_eq!(redact_command_line("\"--secret=a b\""), "\"--secret=****\"");
    }

    #[test]
    fn masks_bare_flag_values_only_once() {
        assert_eq!(
            redact_command_line("app --token --verbose"),
            "app --token --verbose"
        );
        assert_eq!(redact_command_line("app --pass x y"), "app --pass **** y");
    }

    #[test]
    fn masks_known_token_formats() {
        let gh = format!("ghp_{}", "a".repeat(36));
        assert_eq!(redact_command_line(&format!("tool {gh}")), "tool ****");
        assert_eq!(
            redact_command_line(&format!("tool --x={gh}")),
            "tool --x=****"
        );
        let jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjMifQ.c2lnbmF0dXJlLXNpZ25hdHVyZQ";
        assert_eq!(
            redact_command_line(&format!("curl -H {jwt}")),
            "curl -H ****"
        );
    }

    #[test]
    fn masks_url_passwords() {
        assert_eq!(
            redact_command_line("psql postgres://app:s3cret@localhost:5432/db"),
            "psql postgres://app:****@localhost:5432/db"
        );
        assert_eq!(
            redact_command_line("open https://example.com:8443/path"),
            "open https://example.com:8443/path"
        );
        assert_eq!(
            redact_command_line("DATABASE_URL=postgres://a:b@h/db x"),
            "DATABASE_URL=**** x"
        );
    }
}
