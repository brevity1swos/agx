//! Heuristic PII / credential scanner for `--scan-pii`. Reports
//! matches, does not mutate — pair with `--redact` when the intent is
//! to scrub.
//!
//! Pattern coverage is intentionally narrow: well-known credential
//! shapes where a false positive is extremely unlikely, and the
//! public email / phone / IPv4 shapes that come up in agent traces.
//! We stay prefix-based (no `regex` crate) because:
//!
//! 1. The patterns we care about all have unambiguous prefixes.
//! 2. A runtime regex dep adds ~500KB to the default binary and a
//!    build-time hit most users don't need. `--redact` already lives
//!    behind literal-substring masking for the same reason.
//! 3. False-negatives on unusual shapes are acceptable for v1; users
//!    who need regex-powered detection can grep the `--export json`
//!    output themselves.
//!
//! Categories land as `Category` enum variants so JSON output stays
//! stable when we add new patterns (new variants, not renames).

use serde::Serialize;

/// One PII match. `offset` is a char-based index into the input
/// string so callers showing a snippet can safely slice.
#[derive(Debug, Clone, Serialize)]
pub struct Match {
    pub category: Category,
    /// 0-based step index (when scanning a step); synthesized as 0
    /// for free-text scans.
    pub step_index: usize,
    /// Short excerpt of the match plus a few chars on each side, for
    /// human-readable output. Length capped to keep summaries terse.
    pub snippet: String,
}

/// Known PII / credential categories. Serialized as snake_case so the
/// JSON shape is downstream-friendly.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Email,
    Ipv4,
    AwsAccessKey,
    StripeSecretKey,
    StripePublishableKey,
    GithubToken,
    OpenaiKey,
    AnthropicKey,
    SshPrivateKeyHeader,
    JwtToken,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Category::Email => "email",
            Category::Ipv4 => "ipv4",
            Category::AwsAccessKey => "aws_access_key",
            Category::StripeSecretKey => "stripe_secret_key",
            Category::StripePublishableKey => "stripe_publishable_key",
            Category::GithubToken => "github_token",
            Category::OpenaiKey => "openai_key",
            Category::AnthropicKey => "anthropic_key",
            Category::SshPrivateKeyHeader => "ssh_private_key_header",
            Category::JwtToken => "jwt_token",
        }
    }
}

/// Scan a single string for all known PII shapes.
#[must_use]
pub fn scan(text: &str) -> Vec<Match> {
    scan_with_step(text, 0)
}

/// Scan a string associated with a specific step index. The step
/// index is copied into every emitted Match so corpus-level summaries
/// can rank by step position.
#[must_use]
pub fn scan_with_step(text: &str, step_index: usize) -> Vec<Match> {
    let mut out = Vec::new();
    // Prefix patterns — most credential shapes live here. Each entry
    // is (category, prefix, min_tail_chars). A match starts at the
    // prefix and includes the prefix + `min_tail_chars` following
    // alphanumeric / hyphen / underscore bytes. Prefixes are distinct
    // enough across real outputs that this is a very low-false-
    // positive shape.
    const PREFIXES: &[(Category, &str, usize)] = &[
        (Category::AwsAccessKey, "AKIA", 16),
        (Category::AwsAccessKey, "ASIA", 16),
        (Category::StripeSecretKey, "sk_live_", 24),
        (Category::StripeSecretKey, "sk_test_", 24),
        (Category::StripePublishableKey, "pk_live_", 24),
        (Category::StripePublishableKey, "pk_test_", 24),
        (Category::GithubToken, "ghp_", 36),
        (Category::GithubToken, "gho_", 36),
        (Category::GithubToken, "ghu_", 36),
        (Category::GithubToken, "ghs_", 36),
        (Category::GithubToken, "ghr_", 36),
        (Category::AnthropicKey, "sk-ant-", 32),
    ];
    for &(cat, prefix, min_tail) in PREFIXES {
        scan_prefix(text, step_index, cat, prefix, min_tail, &mut out);
    }

    // OpenAI key: `sk-` followed by ≥32 chars, but must NOT start
    // with `sk-ant-` (that's the Anthropic key handled above).
    scan_openai_key(text, step_index, &mut out);

    // Email addresses — minimal heuristic that avoids the regex dep.
    scan_email(text, step_index, &mut out);

    // IPv4 — 4 groups of 1-3 digits separated by dots, each group 0-255.
    scan_ipv4(text, step_index, &mut out);

    // SSH private-key armor strings. Exact markers.
    const SSH_HEADERS: &[&str] = &[
        "-----BEGIN OPENSSH PRIVATE KEY-----",
        "-----BEGIN RSA PRIVATE KEY-----",
        "-----BEGIN DSA PRIVATE KEY-----",
        "-----BEGIN EC PRIVATE KEY-----",
        "-----BEGIN PRIVATE KEY-----",
    ];
    for header in SSH_HEADERS {
        if text.contains(header) {
            out.push(Match {
                category: Category::SshPrivateKeyHeader,
                step_index,
                snippet: (*header).to_string(),
            });
        }
    }

    // JWT tokens: three base64url groups joined by `.`, starting with
    // `eyJ` (the base64 of `{"`). Common in agent tool outputs that
    // call authenticated APIs.
    scan_jwt(text, step_index, &mut out);

    out
}

/// Scan every step's `detail` and `label`, returning all matches
/// indexed by step position. Convenience wrapper for the CLI
/// dispatcher in main.rs.
#[must_use]
pub fn scan_steps(steps: &[crate::timeline::Step]) -> Vec<Match> {
    let mut all = Vec::new();
    for (i, step) in steps.iter().enumerate() {
        all.extend(scan_with_step(&step.detail, i));
        all.extend(scan_with_step(&step.label, i));
    }
    all
}

// ---------- internal helpers ----------

fn is_token_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

fn scan_prefix(
    text: &str,
    step_index: usize,
    cat: Category,
    prefix: &str,
    min_tail: usize,
    out: &mut Vec<Match>,
) {
    let bytes = text.as_bytes();
    let prefix_bytes = prefix.as_bytes();
    let mut i = 0;
    while i + prefix_bytes.len() <= bytes.len() {
        if &bytes[i..i + prefix_bytes.len()] == prefix_bytes {
            // Count trailing token bytes. If enough, emit a match.
            let tail_start = i + prefix_bytes.len();
            let mut tail = 0;
            while tail_start + tail < bytes.len() && is_token_byte(bytes[tail_start + tail]) {
                tail += 1;
            }
            if tail >= min_tail {
                let end = tail_start + tail;
                let snippet = text[i..end].to_string();
                out.push(Match {
                    category: cat,
                    step_index,
                    snippet,
                });
                i = end;
                continue;
            }
        }
        i += 1;
    }
}

fn scan_openai_key(text: &str, step_index: usize, out: &mut Vec<Match>) {
    // Match `sk-` + ≥32 token chars, skip `sk-ant-`.
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        if &bytes[i..i + 3] == b"sk-" {
            // Reject the Anthropic prefix.
            if bytes[i..].starts_with(b"sk-ant-") {
                i += 1;
                continue;
            }
            let tail_start = i + 3;
            let mut tail = 0;
            while tail_start + tail < bytes.len() && is_token_byte(bytes[tail_start + tail]) {
                tail += 1;
            }
            if tail >= 32 {
                let end = tail_start + tail;
                out.push(Match {
                    category: Category::OpenaiKey,
                    step_index,
                    snippet: text[i..end].to_string(),
                });
                i = end;
                continue;
            }
        }
        i += 1;
    }
}

fn scan_email(text: &str, step_index: usize, out: &mut Vec<Match>) {
    // Heuristic: find every `@`, check there's a local-part before
    // and a domain-with-dot after. Not RFC-compliant but catches the
    // shapes that actually show up in agent traces.
    let bytes = text.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'@' {
            continue;
        }
        // Walk backwards for the local part.
        let mut start = i;
        while start > 0 && is_email_local_byte(bytes[start - 1]) {
            start -= 1;
        }
        if start == i {
            continue;
        }
        // Walk forwards for the domain.
        let mut end = i + 1;
        while end < bytes.len() && is_email_domain_byte(bytes[end]) {
            end += 1;
        }
        if end == i + 1 {
            continue;
        }
        // The domain portion must contain a `.`.
        let domain = &text[i + 1..end];
        if !domain.contains('.') {
            continue;
        }
        out.push(Match {
            category: Category::Email,
            step_index,
            snippet: text[start..end].to_string(),
        });
    }
}

fn is_email_local_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'+')
}

fn is_email_domain_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'.' || b == b'-'
}

fn scan_ipv4(text: &str, step_index: usize, out: &mut Vec<Match>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        // Try to parse up to 4 dot-separated octets starting at i.
        if let Some(end) = parse_ipv4_at(bytes, i) {
            out.push(Match {
                category: Category::Ipv4,
                step_index,
                snippet: text[i..end].to_string(),
            });
            i = end;
        } else {
            // Skip past this run of digits.
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
        }
    }
}

fn parse_ipv4_at(bytes: &[u8], start: usize) -> Option<usize> {
    let mut pos = start;
    for seg in 0..4 {
        if pos >= bytes.len() || !bytes[pos].is_ascii_digit() {
            return None;
        }
        let mut digits = 0;
        let mut val: u32 = 0;
        while pos < bytes.len() && bytes[pos].is_ascii_digit() && digits < 3 {
            val = val * 10 + u32::from(bytes[pos] - b'0');
            pos += 1;
            digits += 1;
        }
        if val > 255 {
            return None;
        }
        if seg < 3 {
            if pos >= bytes.len() || bytes[pos] != b'.' {
                return None;
            }
            pos += 1;
        }
    }
    // Reject when immediately followed by another digit — otherwise
    // we'd hit `12.34.56.789` reading as 12.34.56.78 with leftover 9.
    if pos < bytes.len() && bytes[pos].is_ascii_digit() {
        return None;
    }
    Some(pos)
}

fn scan_jwt(text: &str, step_index: usize, out: &mut Vec<Match>) {
    // `eyJ` is the base64url of `{"` — the standard JWT header start.
    // Three groups of base64url chars separated by `.`, each ≥16.
    let bytes = text.as_bytes();
    let mut i = 0;
    while i + 3 <= bytes.len() {
        if &bytes[i..i + 3] == b"eyJ" {
            if let Some(end) = parse_jwt_at(bytes, i) {
                out.push(Match {
                    category: Category::JwtToken,
                    step_index,
                    snippet: text[i..end].to_string(),
                });
                i = end;
                continue;
            }
        }
        i += 1;
    }
}

fn parse_jwt_at(bytes: &[u8], start: usize) -> Option<usize> {
    let mut pos = start;
    for seg in 0..3 {
        let seg_start = pos;
        while pos < bytes.len() && is_base64url_byte(bytes[pos]) {
            pos += 1;
        }
        if pos - seg_start < 16 {
            return None;
        }
        if seg < 2 {
            if pos >= bytes.len() || bytes[pos] != b'.' {
                return None;
            }
            pos += 1;
        }
    }
    Some(pos)
}

fn is_base64url_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_' || b == b'-'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_aws_access_key() {
        let m = scan("export AWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE");
        assert!(m.iter().any(|x| x.category == Category::AwsAccessKey));
    }

    #[test]
    fn finds_stripe_keys() {
        let m = scan("key: sk_live_aaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert!(m.iter().any(|x| x.category == Category::StripeSecretKey));
        let m = scan("pub: pk_test_bbbbbbbbbbbbbbbbbbbbbbbbbb");
        assert!(
            m.iter()
                .any(|x| x.category == Category::StripePublishableKey)
        );
    }

    #[test]
    fn finds_github_tokens() {
        let tok = "ghp_".to_string() + &"a".repeat(36);
        let m = scan(&tok);
        assert!(m.iter().any(|x| x.category == Category::GithubToken));
    }

    #[test]
    fn distinguishes_openai_from_anthropic() {
        let openai = "sk-".to_string() + &"a".repeat(48);
        let anthropic = "sk-ant-".to_string() + &"a".repeat(40);
        let m_o = scan(&openai);
        assert!(m_o.iter().any(|x| x.category == Category::OpenaiKey));
        assert!(!m_o.iter().any(|x| x.category == Category::AnthropicKey));
        let m_a = scan(&anthropic);
        assert!(m_a.iter().any(|x| x.category == Category::AnthropicKey));
        assert!(!m_a.iter().any(|x| x.category == Category::OpenaiKey));
    }

    #[test]
    fn finds_emails() {
        let m = scan("contact alice+test@example.com and bob@x.io");
        let emails: Vec<_> = m.iter().filter(|x| x.category == Category::Email).collect();
        assert_eq!(emails.len(), 2);
    }

    #[test]
    fn rejects_bare_at_without_domain_dot() {
        let m = scan("twitter handle @alice here");
        assert!(!m.iter().any(|x| x.category == Category::Email));
    }

    #[test]
    fn finds_ipv4_but_rejects_out_of_range() {
        let m = scan("connect to 10.0.0.1 and 192.168.1.50");
        let ips: Vec<_> = m.iter().filter(|x| x.category == Category::Ipv4).collect();
        assert_eq!(ips.len(), 2);
        let m = scan("fake 999.999.999.999 and 300.1.1.1");
        assert!(!m.iter().any(|x| x.category == Category::Ipv4));
    }

    #[test]
    fn finds_ssh_private_key_header() {
        let text = "-----BEGIN OPENSSH PRIVATE KEY-----\nfake";
        let m = scan(text);
        assert!(
            m.iter()
                .any(|x| x.category == Category::SshPrivateKeyHeader)
        );
    }

    #[test]
    fn finds_jwt() {
        // Three base64url groups ≥16 chars each, joined by dots.
        let jwt = format!(
            "{}.{}.{}",
            "eyJ".to_string() + &"a".repeat(20),
            "a".repeat(20),
            "a".repeat(20)
        );
        let m = scan(&jwt);
        assert!(m.iter().any(|x| x.category == Category::JwtToken));
    }

    #[test]
    fn rejects_non_jwt_starting_with_eyj() {
        // eyJ followed by non-base64 chars → no match.
        let m = scan("eyJ{not a jwt");
        assert!(!m.iter().any(|x| x.category == Category::JwtToken));
    }

    #[test]
    fn scan_steps_indexes_by_step_position() {
        use crate::timeline::{tool_result_step, user_text_step};
        let steps = vec![
            user_text_step("clean input"),
            tool_result_step(
                "t1",
                "secret AKIAIOSFODNN7EXAMPLE found",
                Some("Bash"),
                None,
            ),
        ];
        let matches = scan_steps(&steps);
        assert!(
            matches
                .iter()
                .any(|m| m.step_index == 1 && m.category == Category::AwsAccessKey)
        );
    }

    #[test]
    fn empty_input_returns_no_matches() {
        assert!(scan("").is_empty());
    }
}
