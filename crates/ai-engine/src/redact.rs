//! C3.4 redaction for anything the Engine logs or returns as an error:
//! removes `sk-…` keys, bearer tokens, `key=`/`token=` values and any value
//! the caller says is a live secret (the current Keychain key).

use std::sync::OnceLock;

use regex::Regex;

fn patterns() -> &'static [Regex] {
    static PATTERNS: OnceLock<Vec<Regex>> = OnceLock::new();
    PATTERNS.get_or_init(|| {
        [
            r"sk-[A-Za-z0-9_\-]{8,}",
            r"(?i)bearer\s+[A-Za-z0-9._~+/=\-]+",
            r"(?i)\b(api[_-]?key|key|token|access_token)=([^&\s]+)",
            r#"(?i)"?x-api-key"?\s*[:=]\s*"?[^\s",}]+"#,
        ]
        .iter()
        .map(|p| Regex::new(p).expect("static regex"))
        .collect()
    })
}

pub fn redact(input: &str, secrets: &[&str]) -> String {
    let mut out = input.to_string();
    for secret in secrets {
        if secret.len() >= 8 {
            out = out.replace(secret, "[redacted]");
        }
    }
    for pattern in patterns() {
        out = pattern.replace_all(&out, "[redacted]").into_owned();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_keys_tokens_and_live_secrets() {
        let text = "bad sk-ant-api03-abcdefghijkl Bearer abc.def x-api-key: zzz \
                    https://h/?key=123&token=456 my-live-secret-value";
        let out = redact(text, &["my-live-secret-value"]);
        for leaked in [
            "sk-ant",
            "abc.def",
            "zzz",
            "123",
            "456",
            "my-live-secret-value",
        ] {
            assert!(!out.contains(leaked), "{leaked} leaked in {out}");
        }
    }
}
