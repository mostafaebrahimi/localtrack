use serde::{Deserialize, Serialize};
use url::Url;

use crate::limits;

/// How much of a URL may be stored (spec §45). Default is `PathWithoutQuery`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum UrlPolicy {
    DomainOnly,
    #[default]
    PathWithoutQuery,
    FullUrl,
}

impl UrlPolicy {
    pub fn as_str(&self) -> &'static str {
        match self {
            UrlPolicy::DomainOnly => "DOMAIN_ONLY",
            UrlPolicy::PathWithoutQuery => "PATH_WITHOUT_QUERY",
            UrlPolicy::FullUrl => "FULL_URL",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "DOMAIN_ONLY" => Some(UrlPolicy::DomainOnly),
            "PATH_WITHOUT_QUERY" => Some(UrlPolicy::PathWithoutQuery),
            "FULL_URL" => Some(UrlPolicy::FullUrl),
            _ => None,
        }
    }
}

/// The registrable host of a URL, lower-cased and without credentials.
pub fn domain_of(raw: &str) -> Option<String> {
    let parsed = Url::parse(raw.trim()).ok()?;
    let host = parsed
        .host_str()?
        .trim_start_matches("www.")
        .to_ascii_lowercase();
    if host.is_empty() {
        None
    } else {
        Some(limits::truncate(&host, limits::DOMAIN_MAX))
    }
}

/// Sanitize a URL according to policy (spec §44).
///
/// Always removes username, password, query string and fragment unless the user
/// explicitly opted into `FULL_URL`; even then credentials are stripped, because
/// a password in a URL must never reach the database.
pub fn sanitize_url(raw: &str, policy: UrlPolicy) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let mut parsed = Url::parse(trimmed).ok()?;

    // Non-web schemes (chrome://, file://, about:) are never stored.
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }

    // Credentials are stripped under every policy.
    let _ = parsed.set_username("");
    let _ = parsed.set_password(None);

    let sanitized = match policy {
        UrlPolicy::DomainOnly => {
            parsed.set_query(None);
            parsed.set_fragment(None);
            parsed.set_path("");
            parsed.as_str().trim_end_matches('/').to_string()
        }
        UrlPolicy::PathWithoutQuery => {
            parsed.set_query(None);
            parsed.set_fragment(None);
            let s = parsed.as_str().to_string();
            if s.ends_with('/') && parsed.path() == "/" {
                s.trim_end_matches('/').to_string()
            } else {
                s
            }
        }
        UrlPolicy::FullUrl => parsed.as_str().to_string(),
    };

    Some(limits::truncate(&sanitized, limits::URL_MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SENSITIVE: &str = "https://user:pw@example.com/orders/183?token=abc&page=2#payment";

    #[test]
    fn default_policy_strips_query_fragment_and_credentials() {
        // Spec §44 canonical example.
        let out = sanitize_url(SENSITIVE, UrlPolicy::PathWithoutQuery).unwrap();
        assert_eq!(out, "https://example.com/orders/183");
        assert!(!out.contains("token"));
        assert!(!out.contains("abc"));
        assert!(!out.contains("pw"));
    }

    #[test]
    fn privacy_acceptance_case_from_spec_138() {
        let out = sanitize_url(
            "https://example.com/login?password=secret&token=123",
            UrlPolicy::PathWithoutQuery,
        )
        .unwrap();
        assert_eq!(out, "https://example.com/login");
        assert!(!out.contains("secret"));
        assert!(!out.contains("123"));
    }

    #[test]
    fn domain_only_policy() {
        assert_eq!(
            sanitize_url(SENSITIVE, UrlPolicy::DomainOnly).unwrap(),
            "https://example.com"
        );
    }

    #[test]
    fn full_url_keeps_query_but_never_credentials() {
        let out = sanitize_url(SENSITIVE, UrlPolicy::FullUrl).unwrap();
        assert!(out.contains("token=abc"));
        assert!(!out.contains("user:pw"));
    }

    #[test]
    fn non_web_schemes_are_dropped() {
        assert!(sanitize_url("chrome://extensions", UrlPolicy::FullUrl).is_none());
        assert!(sanitize_url("file:///home/user/secret.txt", UrlPolicy::FullUrl).is_none());
        assert!(sanitize_url("about:blank", UrlPolicy::FullUrl).is_none());
        assert!(sanitize_url("", UrlPolicy::FullUrl).is_none());
    }

    #[test]
    fn domain_extraction_normalizes() {
        assert_eq!(
            domain_of("https://WWW.GitHub.com/a/b"),
            Some("github.com".into())
        );
        assert_eq!(domain_of("not a url"), None);
    }

    #[test]
    fn policy_round_trip() {
        for p in [
            UrlPolicy::DomainOnly,
            UrlPolicy::PathWithoutQuery,
            UrlPolicy::FullUrl,
        ] {
            assert_eq!(UrlPolicy::parse(p.as_str()), Some(p));
        }
    }
}
