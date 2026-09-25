//! Filters and pagination for activity queries (spec §81, §121).
//!
//! Filters are compiled into parameterized SQL — values are never interpolated
//! into the statement text (spec §107).

use rusqlite::types::Value;
use serde::{Deserialize, Serialize};

/// Default page size for the activity list (spec §121).
pub const DEFAULT_PAGE_SIZE: i64 = 100;
pub const MAX_PAGE_SIZE: i64 = 1000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum SortOrder {
    #[default]
    Asc,
    Desc,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub limit: i64,
    pub offset: i64,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            limit: DEFAULT_PAGE_SIZE,
            offset: 0,
        }
    }
}

impl Page {
    pub fn clamped(&self) -> Page {
        Page {
            limit: self.limit.clamp(1, MAX_PAGE_SIZE),
            offset: self.offset.max(0),
        }
    }
}

/// Activity query filters.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ActivityFilter {
    pub from_ms: Option<i64>,
    pub to_ms: Option<i64>,

    pub sources: Vec<String>,
    pub kinds: Vec<String>,

    pub app_names: Vec<String>,
    pub process_names: Vec<String>,
    pub browsers: Vec<String>,
    pub domains: Vec<String>,

    pub category_ids: Vec<String>,
    pub project_ids: Vec<String>,

    /// `Some(true)` keeps only idle/locked activity, `Some(false)` only active.
    pub is_afk: Option<bool>,

    pub min_duration_ms: Option<i64>,
    pub max_duration_ms: Option<i64>,

    /// Free-text search across application, title, domain, page title and URL.
    pub search: Option<String>,

    /// Only segments with no category / no project.
    pub uncategorized_only: bool,
    pub unassigned_only: bool,
}

/// A compiled `WHERE` clause with its bound parameters.
pub struct CompiledFilter {
    pub where_sql: String,
    pub params: Vec<Value>,
}

impl ActivityFilter {
    pub fn for_range(from_ms: i64, to_ms: i64) -> Self {
        Self {
            from_ms: Some(from_ms),
            to_ms: Some(to_ms),
            ..Default::default()
        }
    }

    /// Compile to parameterized SQL. `next_index` is the first `?N` to use.
    pub fn compile(&self, next_index: usize) -> CompiledFilter {
        let mut clauses: Vec<String> = Vec::new();
        let mut params: Vec<Value> = Vec::new();
        let mut index = next_index;

        let mut push = |clause: String, values: Vec<Value>, index: &mut usize| {
            clauses.push(clause);
            *index += values.len();
            params.extend(values);
        };

        // Overlap test: a segment counts when it intersects the window.
        if let Some(to) = self.to_ms {
            push(
                format!("started_at_ms < ?{index}"),
                vec![Value::Integer(to)],
                &mut index,
            );
        }
        if let Some(from) = self.from_ms {
            push(
                format!("ended_at_ms > ?{index}"),
                vec![Value::Integer(from)],
                &mut index,
            );
        }

        let in_clause = |column: &str,
                         values: &Vec<String>,
                         clauses: &mut Vec<String>,
                         params: &mut Vec<Value>,
                         index: &mut usize| {
            if values.is_empty() {
                return;
            }
            let placeholders: Vec<String> = values
                .iter()
                .enumerate()
                .map(|(offset, _)| format!("?{}", *index + offset))
                .collect();
            clauses.push(format!("{column} IN ({})", placeholders.join(", ")));
            for value in values {
                params.push(Value::Text(value.clone()));
            }
            *index += values.len();
        };

        in_clause(
            "source",
            &self.sources,
            &mut clauses,
            &mut params,
            &mut index,
        );
        in_clause("kind", &self.kinds, &mut clauses, &mut params, &mut index);
        in_clause(
            "app_name",
            &self.app_names,
            &mut clauses,
            &mut params,
            &mut index,
        );
        in_clause(
            "process_name",
            &self.process_names,
            &mut clauses,
            &mut params,
            &mut index,
        );
        in_clause(
            "browser",
            &self.browsers,
            &mut clauses,
            &mut params,
            &mut index,
        );
        in_clause(
            "domain",
            &self.domains,
            &mut clauses,
            &mut params,
            &mut index,
        );
        in_clause(
            "category_id",
            &self.category_ids,
            &mut clauses,
            &mut params,
            &mut index,
        );
        in_clause(
            "project_id",
            &self.project_ids,
            &mut clauses,
            &mut params,
            &mut index,
        );

        if let Some(afk) = self.is_afk {
            clauses.push(format!("is_afk = ?{index}"));
            params.push(Value::Integer(i64::from(afk)));
            index += 1;
        }

        if let Some(min) = self.min_duration_ms {
            clauses.push(format!("(ended_at_ms - started_at_ms) >= ?{index}"));
            params.push(Value::Integer(min));
            index += 1;
        }
        if let Some(max) = self.max_duration_ms {
            clauses.push(format!("(ended_at_ms - started_at_ms) <= ?{index}"));
            params.push(Value::Integer(max));
            index += 1;
        }

        if self.uncategorized_only {
            clauses.push("category_id IS NULL".to_string());
        }
        if self.unassigned_only {
            clauses.push("project_id IS NULL".to_string());
        }

        if let Some(search) = self
            .search
            .as_ref()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
        {
            // LIKE with an escaped pattern; the value stays a bound parameter.
            let pattern = format!(
                "%{}%",
                search
                    .replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            );
            clauses.push(format!(
                "(COALESCE(app_name,'') LIKE ?{i} ESCAPE '\\'
                  OR COALESCE(process_name,'') LIKE ?{i} ESCAPE '\\'
                  OR COALESCE(window_title,'') LIKE ?{i} ESCAPE '\\'
                  OR COALESCE(domain,'') LIKE ?{i} ESCAPE '\\'
                  OR COALESCE(page_title,'') LIKE ?{i} ESCAPE '\\'
                  OR COALESCE(url,'') LIKE ?{i} ESCAPE '\\')",
                i = index
            ));
            params.push(Value::Text(pattern));
            index += 1;
        }

        let _ = index;
        let where_sql = if clauses.is_empty() {
            "1 = 1".to_string()
        } else {
            clauses.join(" AND ")
        };

        CompiledFilter { where_sql, params }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_filter_matches_everything() {
        let compiled = ActivityFilter::default().compile(1);
        assert_eq!(compiled.where_sql, "1 = 1");
        assert!(compiled.params.is_empty());
    }

    #[test]
    fn range_filter_uses_overlap_semantics() {
        let compiled = ActivityFilter::for_range(10, 20).compile(1);
        assert_eq!(
            compiled.where_sql,
            "started_at_ms < ?1 AND ended_at_ms > ?2"
        );
        assert_eq!(
            compiled.params,
            vec![Value::Integer(20), Value::Integer(10)]
        );
    }

    #[test]
    fn in_clauses_bind_every_value() {
        let filter = ActivityFilter {
            app_names: vec!["Code".into(), "Chrome".into()],
            ..Default::default()
        };
        let compiled = filter.compile(1);
        assert_eq!(compiled.where_sql, "app_name IN (?1, ?2)");
        assert_eq!(compiled.params.len(), 2);
    }

    #[test]
    fn search_values_are_never_inlined() {
        let filter = ActivityFilter {
            search: Some("'; DROP TABLE activity_segments; --".into()),
            ..Default::default()
        };
        let compiled = filter.compile(1);
        assert!(!compiled.where_sql.contains("DROP"));
        assert_eq!(compiled.params.len(), 1);
    }

    #[test]
    fn page_is_clamped() {
        assert_eq!(
            Page {
                limit: 0,
                offset: -5
            }
            .clamped(),
            Page {
                limit: 1,
                offset: 0
            }
        );
        assert_eq!(
            Page {
                limit: 99999,
                offset: 0
            }
            .clamped()
            .limit,
            MAX_PAGE_SIZE
        );
    }
}
