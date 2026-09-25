//! Rule-based classification into categories and projects (spec §60–§66).

use std::borrow::Cow;
use std::collections::HashMap;

use regex::RegexBuilder;
use serde::{Deserialize, Serialize};

use crate::activity::{ActivitySegment, ClassificationSource};
use crate::error::{CoreError, Result};
use crate::privacy::exclusion::glob_match;

/// Fields a rule can target (spec §61).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleField {
    AppName,
    ProcessName,
    WindowTitle,
    Browser,
    Domain,
    Url,
    PageTitle,
    /// The project, directory or task read out of the window title
    /// (see [`crate::context`]). Rules can name real work rather than
    /// applications: `workspace is "helpdesk-v2"` instead of `app is "Code"`.
    Workspace,
}

impl RuleField {
    pub fn as_str(&self) -> &'static str {
        match self {
            RuleField::AppName => "app_name",
            RuleField::ProcessName => "process_name",
            RuleField::WindowTitle => "window_title",
            RuleField::Browser => "browser",
            RuleField::Domain => "domain",
            RuleField::Url => "url",
            RuleField::PageTitle => "page_title",
            RuleField::Workspace => "workspace",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_lowercase().as_str() {
            "app_name" => Some(RuleField::AppName),
            "process_name" => Some(RuleField::ProcessName),
            "window_title" => Some(RuleField::WindowTitle),
            "browser" => Some(RuleField::Browser),
            "domain" => Some(RuleField::Domain),
            "url" => Some(RuleField::Url),
            "page_title" => Some(RuleField::PageTitle),
            "workspace" => Some(RuleField::Workspace),
            _ => None,
        }
    }

    /// The value a rule tests.
    ///
    /// Most fields are read straight off the segment; the workspace is derived
    /// from the title, which is why this hands back a [`Cow`] rather than a
    /// borrow.
    pub fn value_of<'a>(&self, segment: &'a ActivitySegment) -> Option<Cow<'a, str>> {
        self.borrowed(segment)
            .map(Cow::Borrowed)
            .or_else(|| match self {
                RuleField::Workspace => crate::context::work_context(segment)
                    .workspace
                    .map(Cow::Owned),
                _ => None,
            })
    }

    fn borrowed<'a>(&self, segment: &'a ActivitySegment) -> Option<&'a str> {
        match self {
            RuleField::AppName => segment.app_name.as_deref(),
            RuleField::ProcessName => segment.process_name.as_deref(),
            RuleField::WindowTitle => segment.window_title.as_deref(),
            RuleField::Browser => segment.browser.as_deref(),
            RuleField::Domain => segment.domain.as_deref(),
            RuleField::Url => segment.url.as_deref(),
            RuleField::PageTitle => segment.page_title.as_deref(),
            RuleField::Workspace => None,
        }
    }
}

/// Match operators (spec §62). All are case-insensitive by default.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RuleOperator {
    Exact,
    Contains,
    StartsWith,
    EndsWith,
    Glob,
    Regex,
}

impl RuleOperator {
    pub fn as_str(&self) -> &'static str {
        match self {
            RuleOperator::Exact => "EXACT",
            RuleOperator::Contains => "CONTAINS",
            RuleOperator::StartsWith => "STARTS_WITH",
            RuleOperator::EndsWith => "ENDS_WITH",
            RuleOperator::Glob => "GLOB",
            RuleOperator::Regex => "REGEX",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value.to_ascii_uppercase().as_str() {
            "EXACT" => Some(RuleOperator::Exact),
            "CONTAINS" => Some(RuleOperator::Contains),
            "STARTS_WITH" => Some(RuleOperator::StartsWith),
            "ENDS_WITH" => Some(RuleOperator::EndsWith),
            "GLOB" => Some(RuleOperator::Glob),
            "REGEX" => Some(RuleOperator::Regex),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassificationRule {
    pub id: String,
    pub name: String,
    pub enabled: bool,
    /// Larger number = higher priority (spec §64).
    pub priority: i64,
    pub target_field: RuleField,
    pub operator: RuleOperator,
    pub pattern: String,
    pub category_id: Option<String>,
    pub project_id: Option<String>,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
}

impl ClassificationRule {
    /// Reject a rule whose pattern cannot compile before it is stored.
    pub fn validate(&self) -> Result<()> {
        if self.pattern.trim().is_empty() {
            return Err(CoreError::InvalidPattern(
                "pattern must not be empty".into(),
            ));
        }
        if self.operator == RuleOperator::Regex {
            RegexBuilder::new(&self.pattern)
                .case_insensitive(true)
                .size_limit(1 << 20)
                .build()
                .map_err(|e| CoreError::InvalidPattern(e.to_string()))?;
        }
        if self.category_id.is_none() && self.project_id.is_none() {
            return Err(CoreError::Validation(
                "a rule must assign a category, a project, or both".into(),
            ));
        }
        Ok(())
    }
}

/// Result of classifying a segment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Classification {
    pub category_id: Option<String>,
    pub project_id: Option<String>,
    pub source: ClassificationSource,
    pub rule_id: Option<String>,
}

impl Classification {
    pub fn default_uncategorized(category_id: Option<String>) -> Self {
        Self {
            category_id,
            project_id: None,
            source: ClassificationSource::Default,
            rule_id: None,
        }
    }
}

/// Compiled rule set, sorted by priority (highest first).
#[derive(Debug, Default)]
pub struct Classifier {
    rules: Vec<ClassificationRule>,
    regexes: HashMap<String, regex::Regex>,
    default_category_id: Option<String>,
}

impl Classifier {
    pub fn new(rules: Vec<ClassificationRule>, default_category_id: Option<String>) -> Self {
        let mut enabled: Vec<ClassificationRule> =
            rules.into_iter().filter(|r| r.enabled).collect();
        // Highest priority first; ties resolved by most recently updated.
        enabled.sort_by(|a, b| {
            b.priority
                .cmp(&a.priority)
                .then(b.updated_at_ms.cmp(&a.updated_at_ms))
                .then(a.id.cmp(&b.id))
        });

        let mut regexes = HashMap::new();
        for rule in &enabled {
            if rule.operator == RuleOperator::Regex {
                if let Ok(re) = RegexBuilder::new(&rule.pattern)
                    .case_insensitive(true)
                    .size_limit(1 << 20)
                    .build()
                {
                    regexes.insert(rule.id.clone(), re);
                }
            }
        }

        Self {
            rules: enabled,
            regexes,
            default_category_id,
        }
    }

    pub fn rules(&self) -> &[ClassificationRule] {
        &self.rules
    }

    fn matches(&self, rule: &ClassificationRule, value: &str) -> bool {
        let v = value.to_lowercase();
        let p = rule.pattern.to_lowercase();
        match rule.operator {
            RuleOperator::Exact => v == p,
            RuleOperator::Contains => v.contains(&p),
            RuleOperator::StartsWith => v.starts_with(&p),
            RuleOperator::EndsWith => v.ends_with(&p),
            RuleOperator::Glob => glob_match(&rule.pattern, value),
            RuleOperator::Regex => self
                .regexes
                .get(&rule.id)
                .map(|re| re.is_match(value))
                .unwrap_or(false),
        }
    }

    /// Find the first (highest priority) matching rule.
    pub fn first_match(&self, segment: &ActivitySegment) -> Option<&ClassificationRule> {
        self.rules.iter().find(|rule| {
            rule.target_field
                .value_of(segment)
                .map(|value| self.matches(rule, value.as_ref()))
                .unwrap_or(false)
        })
    }

    /// Classify a segment (spec §64 resolution order).
    ///
    /// A manual override is never replaced automatically (spec §66).
    pub fn classify(&self, segment: &ActivitySegment) -> Classification {
        if segment.classification_source == Some(ClassificationSource::Manual) {
            return Classification {
                category_id: segment.category_id.clone(),
                project_id: segment.project_id.clone(),
                source: ClassificationSource::Manual,
                rule_id: None,
            };
        }

        match self.first_match(segment) {
            Some(rule) => Classification {
                category_id: rule
                    .category_id
                    .clone()
                    .or_else(|| self.default_category_id.clone()),
                project_id: rule.project_id.clone(),
                source: ClassificationSource::Rule,
                rule_id: Some(rule.id.clone()),
            },
            None => Classification::default_uncategorized(self.default_category_id.clone()),
        }
    }

    /// Apply classification in place, preserving manual overrides.
    pub fn apply(&self, segment: &mut ActivitySegment) -> bool {
        if segment.classification_source == Some(ClassificationSource::Manual) {
            return false;
        }
        let result = self.classify(segment);
        let changed = segment.category_id != result.category_id
            || segment.project_id != result.project_id
            || segment.classification_source != Some(result.source);
        segment.category_id = result.category_id;
        segment.project_id = result.project_id;
        segment.classification_source = Some(result.source);
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::activity::{ActivityKind, ActivitySource, SegmentKey};

    fn seg_url(url: &str) -> ActivitySegment {
        let key = SegmentKey::browser_page(
            Some("chrome".into()),
            crate::privacy::domain_of(url),
            Some(url.into()),
            Some("title".into()),
        );
        ActivitySegment::from_key(&key, 0, 1000, 0)
    }

    fn seg_app(app: &str) -> ActivitySegment {
        let key = SegmentKey::desktop(
            Some(app.into()),
            Some(format!("{app}.exe")),
            Some("t".into()),
        );
        ActivitySegment::from_key(&key, 0, 1000, 0)
    }

    fn rule(
        id: &str,
        priority: i64,
        field: RuleField,
        op: RuleOperator,
        pattern: &str,
        category: &str,
    ) -> ClassificationRule {
        ClassificationRule {
            id: id.into(),
            name: id.into(),
            enabled: true,
            priority,
            target_field: field,
            operator: op,
            pattern: pattern.into(),
            category_id: Some(category.into()),
            project_id: None,
            created_at_ms: 0,
            updated_at_ms: 0,
        }
    }

    #[test]
    fn spec_63_example_rule() {
        let mut r = rule(
            "company-github",
            10,
            RuleField::Url,
            RuleOperator::StartsWith,
            "https://github.com/company/",
            "development",
        );
        r.project_id = Some("hub".into());
        let c = Classifier::new(vec![r], Some("uncategorized".into()));
        let out = c.classify(&seg_url("https://github.com/company/hub/pull/51"));
        assert_eq!(out.category_id.as_deref(), Some("development"));
        assert_eq!(out.project_id.as_deref(), Some("hub"));
        assert_eq!(out.source, ClassificationSource::Rule);
    }

    #[test]
    fn higher_priority_wins() {
        let low = rule(
            "low",
            1,
            RuleField::Domain,
            RuleOperator::Exact,
            "github.com",
            "research",
        );
        let high = rule(
            "high",
            50,
            RuleField::Domain,
            RuleOperator::Exact,
            "github.com",
            "development",
        );
        let c = Classifier::new(vec![low, high], None);
        assert_eq!(
            c.classify(&seg_url("https://github.com/a"))
                .category_id
                .as_deref(),
            Some("development")
        );
    }

    #[test]
    fn no_match_falls_back_to_default_category() {
        let c = Classifier::new(vec![], Some("uncategorized".into()));
        let out = c.classify(&seg_app("Notepad"));
        assert_eq!(out.category_id.as_deref(), Some("uncategorized"));
        assert_eq!(out.source, ClassificationSource::Default);
    }

    #[test]
    fn manual_override_is_never_replaced() {
        let c = Classifier::new(
            vec![rule(
                "r",
                1,
                RuleField::AppName,
                RuleOperator::Contains,
                "code",
                "development",
            )],
            None,
        );
        let mut segment = seg_app("VS Code");
        segment.category_id = Some("personal".into());
        segment.classification_source = Some(ClassificationSource::Manual);
        assert!(!c.apply(&mut segment));
        assert_eq!(segment.category_id.as_deref(), Some("personal"));
    }

    #[test]
    fn all_operators_work() {
        let cases = [
            (RuleOperator::Exact, "github.com", true),
            (RuleOperator::Contains, "hub", true),
            (RuleOperator::StartsWith, "git", true),
            (RuleOperator::EndsWith, ".com", true),
            (RuleOperator::Glob, "git*.com", true),
            (RuleOperator::Regex, r"^git(hub|lab)\.com$", true),
            (RuleOperator::Exact, "gitlab.com", false),
        ];
        for (op, pattern, expected) in cases {
            let c = Classifier::new(
                vec![rule("r", 1, RuleField::Domain, op, pattern, "development")],
                None,
            );
            let matched = c.first_match(&seg_url("https://github.com/x")).is_some();
            assert_eq!(matched, expected, "operator {op:?} pattern {pattern}");
        }
    }

    #[test]
    fn matching_is_case_insensitive() {
        let c = Classifier::new(
            vec![rule(
                "r",
                1,
                RuleField::AppName,
                RuleOperator::Exact,
                "VISUAL studio code",
                "dev",
            )],
            None,
        );
        assert!(c.first_match(&seg_app("Visual Studio Code")).is_some());
    }

    #[test]
    fn invalid_rules_are_rejected() {
        let mut r = rule(
            "r",
            1,
            RuleField::Domain,
            RuleOperator::Regex,
            "(unclosed",
            "dev",
        );
        assert!(r.validate().is_err());
        r.operator = RuleOperator::Exact;
        assert!(r.validate().is_ok());
        r.pattern = "  ".into();
        assert!(r.validate().is_err());
        r.pattern = "github.com".into();
        r.category_id = None;
        r.project_id = None;
        assert!(r.validate().is_err());
    }

    #[test]
    fn disabled_rules_do_not_match() {
        let mut r = rule(
            "r",
            1,
            RuleField::Domain,
            RuleOperator::Exact,
            "github.com",
            "dev",
        );
        r.enabled = false;
        let c = Classifier::new(vec![r], None);
        assert!(c.first_match(&seg_url("https://github.com/x")).is_none());
    }

    #[test]
    fn idle_segments_are_not_classified_by_app_rules() {
        let c = Classifier::new(
            vec![rule(
                "r",
                1,
                RuleField::AppName,
                RuleOperator::Contains,
                "a",
                "dev",
            )],
            None,
        );
        let idle = ActivitySegment::from_key(&SegmentKey::idle(), 0, 100, 0);
        assert_eq!(idle.kind, ActivityKind::Idle);
        assert_eq!(idle.source, ActivitySource::System);
        assert!(c.first_match(&idle).is_none());
    }
}
