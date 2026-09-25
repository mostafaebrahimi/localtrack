use serde::{Deserialize, Serialize};

use crate::activity::SegmentKey;

/// What a rule inspects (spec §67).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ExclusionTarget {
    Domain,
    Url,
    App,
    Process,
    Title,
}

impl ExclusionTarget {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExclusionTarget::Domain => "DOMAIN",
            ExclusionTarget::Url => "URL",
            ExclusionTarget::App => "APP",
            ExclusionTarget::Process => "PROCESS",
            ExclusionTarget::Title => "TITLE",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "DOMAIN" => Some(ExclusionTarget::Domain),
            "URL" => Some(ExclusionTarget::Url),
            "APP" => Some(ExclusionTarget::App),
            "PROCESS" => Some(ExclusionTarget::Process),
            "TITLE" => Some(ExclusionTarget::Title),
            _ => None,
        }
    }
}

/// What happens when a rule matches (spec §46, §47, §67).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ExclusionAction {
    /// Keep the activity but drop identifying metadata.
    Redact,
    /// Keep only the duration under a generic label.
    DurationOnly,
    /// Store nothing at all.
    Ignore,
}

impl ExclusionAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            ExclusionAction::Ignore => "IGNORE",
            ExclusionAction::DurationOnly => "DURATION_ONLY",
            ExclusionAction::Redact => "REDACT",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "IGNORE" => Some(ExclusionAction::Ignore),
            "DURATION_ONLY" => Some(ExclusionAction::DurationOnly),
            "REDACT" => Some(ExclusionAction::Redact),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExclusionRule {
    pub id: String,
    pub enabled: bool,
    pub target: ExclusionTarget,
    pub pattern: String,
    pub action: ExclusionAction,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

/// Outcome of evaluating every exclusion rule against one observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExclusionDecision {
    Allow,
    Redact,
    DurationOnly,
    Ignore,
}

/// Generic labels used when only duration may be kept.
pub const EXCLUDED_WEBSITE_LABEL: &str = "Excluded Website";
pub const EXCLUDED_APP_LABEL: &str = "Excluded Application";
pub const REDACTED_LABEL: &str = "[redacted]";

/// Case-insensitive glob supporting `*` (any run) and `?` (single character).
pub fn glob_match(pattern: &str, value: &str) -> bool {
    let p: Vec<char> = pattern.to_lowercase().chars().collect();
    let v: Vec<char> = value.to_lowercase().chars().collect();
    let (mut pi, mut vi) = (0usize, 0usize);
    let (mut star, mut mark) = (usize::MAX, 0usize);

    while vi < v.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == v[vi]) {
            pi += 1;
            vi += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = pi;
            mark = vi;
            pi += 1;
        } else if star != usize::MAX {
            pi = star + 1;
            mark += 1;
            vi = mark;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

/// The values a rule may be tested against.
#[derive(Debug, Clone, Default)]
pub struct ExclusionCandidate {
    pub domain: Option<String>,
    pub url: Option<String>,
    pub app_name: Option<String>,
    pub process_name: Option<String>,
    pub title: Option<String>,
}

impl ExclusionCandidate {
    pub fn from_key(key: &SegmentKey) -> Self {
        Self {
            domain: key.domain.clone(),
            url: key.url.clone(),
            app_name: key.app_name.clone(),
            process_name: key.process_name.clone(),
            title: key.window_title.clone().or_else(|| key.page_title.clone()),
        }
    }
}

/// Evaluates exclusion rules. The strictest matching action wins.
#[derive(Debug, Clone, Default)]
pub struct ExclusionMatcher {
    rules: Vec<ExclusionRule>,
}

impl ExclusionMatcher {
    pub fn new(rules: Vec<ExclusionRule>) -> Self {
        Self {
            rules: rules.into_iter().filter(|r| r.enabled).collect(),
        }
    }

    pub fn rules(&self) -> &[ExclusionRule] {
        &self.rules
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    fn matches(rule: &ExclusionRule, candidate: &ExclusionCandidate) -> bool {
        let value = match rule.target {
            ExclusionTarget::Domain => candidate.domain.as_deref(),
            ExclusionTarget::Url => candidate.url.as_deref(),
            ExclusionTarget::App => candidate.app_name.as_deref(),
            ExclusionTarget::Process => candidate.process_name.as_deref(),
            ExclusionTarget::Title => candidate.title.as_deref(),
        };
        let Some(value) = value else { return false };
        let pattern = rule.pattern.trim();
        if pattern.is_empty() {
            return false;
        }

        if glob_match(pattern, value) {
            return true;
        }

        match rule.target {
            // `example.com` also covers its subdomains.
            ExclusionTarget::Domain => {
                let v = value.to_lowercase();
                let p = pattern.to_lowercase();
                v == p || v.ends_with(&format!(".{p}"))
            }
            // `bank.example.com/*` should also match a stored URL that includes
            // the scheme, and a bare domain pattern should match its URLs.
            ExclusionTarget::Url => {
                let v = value.to_lowercase();
                let p = pattern.to_lowercase();
                let stripped = v
                    .trim_start_matches("https://")
                    .trim_start_matches("http://")
                    .to_string();
                glob_match(&p, &stripped) || stripped.starts_with(&p)
            }
            _ => false,
        }
    }

    /// Evaluate all rules; strictest action wins.
    pub fn evaluate(&self, candidate: &ExclusionCandidate) -> ExclusionDecision {
        let mut decision: Option<ExclusionAction> = None;
        for rule in &self.rules {
            if Self::matches(rule, candidate) {
                decision = Some(match decision {
                    Some(existing) => existing.max(rule.action),
                    None => rule.action,
                });
            }
        }
        match decision {
            None => ExclusionDecision::Allow,
            Some(ExclusionAction::Ignore) => ExclusionDecision::Ignore,
            Some(ExclusionAction::DurationOnly) => ExclusionDecision::DurationOnly,
            Some(ExclusionAction::Redact) => ExclusionDecision::Redact,
        }
    }

    /// Apply the decision to a segment key.
    ///
    /// `record_excluded_duration` (spec §46, default false) decides whether a
    /// `DURATION_ONLY` match survives at all.
    pub fn apply(&self, key: SegmentKey, record_excluded_duration: bool) -> Option<SegmentKey> {
        let candidate = ExclusionCandidate::from_key(&key);
        match self.evaluate(&candidate) {
            ExclusionDecision::Allow => Some(key),
            ExclusionDecision::Ignore => None,
            ExclusionDecision::DurationOnly => {
                if !record_excluded_duration {
                    return None;
                }
                let is_browser = key.domain.is_some() || key.url.is_some();
                let mut redacted = SegmentKey {
                    source: key.source.clone(),
                    kind: key.kind.clone(),
                    is_afk: key.is_afk,
                    ..Default::default()
                };
                if is_browser {
                    redacted.browser = key.browser.clone();
                    redacted.domain = Some(EXCLUDED_WEBSITE_LABEL.to_string());
                } else {
                    redacted.app_name = Some(EXCLUDED_APP_LABEL.to_string());
                }
                Some(redacted)
            }
            ExclusionDecision::Redact => {
                let mut redacted = key.clone();
                redacted.url = None;
                redacted.page_title = key.page_title.as_ref().map(|_| REDACTED_LABEL.to_string());
                redacted.window_title = key
                    .window_title
                    .as_ref()
                    .map(|_| REDACTED_LABEL.to_string());
                Some(redacted)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(target: ExclusionTarget, pattern: &str, action: ExclusionAction) -> ExclusionRule {
        ExclusionRule {
            id: format!("rule-{pattern}"),
            enabled: true,
            target,
            pattern: pattern.to_string(),
            action,
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }

    #[test]
    fn glob_basics() {
        assert!(glob_match(
            "bank.example.com/*",
            "bank.example.com/accounts"
        ));
        assert!(glob_match("*.example.com", "mail.example.com"));
        assert!(glob_match("1Password.exe", "1password.EXE"));
        assert!(!glob_match("bank.example.com/*", "example.com/bank"));
        assert!(glob_match("*", "anything"));
    }

    #[test]
    fn excluded_domain_is_never_stored() {
        let matcher = ExclusionMatcher::new(vec![rule(
            ExclusionTarget::Domain,
            "bank.example.com",
            ExclusionAction::Ignore,
        )]);
        let key = SegmentKey::browser_page(
            Some("chrome".into()),
            Some("bank.example.com".into()),
            Some("https://bank.example.com/accounts".into()),
            Some("My accounts".into()),
        );
        assert!(matcher.apply(key, true).is_none());
    }

    #[test]
    fn subdomains_are_covered() {
        let matcher = ExclusionMatcher::new(vec![rule(
            ExclusionTarget::Domain,
            "example.com",
            ExclusionAction::Ignore,
        )]);
        let candidate = ExclusionCandidate {
            domain: Some("secure.example.com".into()),
            ..Default::default()
        };
        assert_eq!(matcher.evaluate(&candidate), ExclusionDecision::Ignore);
    }

    #[test]
    fn url_pattern_matches_stored_url() {
        let matcher = ExclusionMatcher::new(vec![rule(
            ExclusionTarget::Url,
            "example.com/private/*",
            ExclusionAction::Ignore,
        )]);
        let candidate = ExclusionCandidate {
            url: Some("https://example.com/private/notes".into()),
            ..Default::default()
        };
        assert_eq!(matcher.evaluate(&candidate), ExclusionDecision::Ignore);
    }

    #[test]
    fn duration_only_requires_opt_in() {
        let matcher = ExclusionMatcher::new(vec![rule(
            ExclusionTarget::Process,
            "KeePassXC",
            ExclusionAction::DurationOnly,
        )]);
        let key = SegmentKey::desktop(
            Some("KeePassXC".into()),
            Some("KeePassXC".into()),
            Some("Vault - passwords.kdbx".into()),
        );
        assert!(matcher.apply(key.clone(), false).is_none());
        let kept = matcher.apply(key, true).unwrap();
        assert_eq!(kept.app_name.as_deref(), Some(EXCLUDED_APP_LABEL));
        assert!(kept.window_title.is_none());
        assert!(kept.process_name.is_none());
    }

    #[test]
    fn redaction_keeps_domain_but_drops_url_and_title() {
        let matcher = ExclusionMatcher::new(vec![rule(
            ExclusionTarget::Domain,
            "mail.example.com",
            ExclusionAction::Redact,
        )]);
        let key = SegmentKey::browser_page(
            Some("chrome".into()),
            Some("mail.example.com".into()),
            Some("https://mail.example.com/inbox/1".into()),
            Some("Invoice from Bob".into()),
        );
        let out = matcher.apply(key, false).unwrap();
        assert_eq!(out.domain.as_deref(), Some("mail.example.com"));
        assert!(out.url.is_none());
        assert_eq!(out.page_title.as_deref(), Some(REDACTED_LABEL));
    }

    #[test]
    fn strictest_action_wins() {
        let matcher = ExclusionMatcher::new(vec![
            rule(
                ExclusionTarget::Domain,
                "example.com",
                ExclusionAction::Redact,
            ),
            rule(
                ExclusionTarget::Domain,
                "example.com",
                ExclusionAction::Ignore,
            ),
        ]);
        let candidate = ExclusionCandidate {
            domain: Some("example.com".into()),
            ..Default::default()
        };
        assert_eq!(matcher.evaluate(&candidate), ExclusionDecision::Ignore);
    }

    #[test]
    fn disabled_rules_are_ignored() {
        let mut r = rule(
            ExclusionTarget::Domain,
            "example.com",
            ExclusionAction::Ignore,
        );
        r.enabled = false;
        let matcher = ExclusionMatcher::new(vec![r]);
        let candidate = ExclusionCandidate {
            domain: Some("example.com".into()),
            ..Default::default()
        };
        assert_eq!(matcher.evaluate(&candidate), ExclusionDecision::Allow);
    }
}
