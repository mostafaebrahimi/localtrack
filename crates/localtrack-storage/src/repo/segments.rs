use localtrack_core::activity::{
    ActivityKind, ActivitySegment, ActivitySource, ClassificationSource,
};
use rusqlite::{params, params_from_iter, Connection, Row, Transaction};

use crate::error::{Result, StorageError};
use crate::filters::{ActivityFilter, Page, SortOrder};

const COLUMNS: &str = "id, source, kind, started_at_ms, ended_at_ms, timezone_offset_min, \
     app_name, process_name, window_title, browser, domain, url, page_title, interaction_type, \
     category_id, project_id, classification_source, is_afk, metadata_json, created_at_ms, updated_at_ms";

fn map_row(row: &Row<'_>) -> rusqlite::Result<ActivitySegment> {
    let source: String = row.get(1)?;
    let kind: String = row.get(2)?;
    let classification: Option<String> = row.get(16)?;
    Ok(ActivitySegment {
        id: row.get(0)?,
        source: ActivitySource::parse(&source).unwrap_or(ActivitySource::Desktop),
        kind: ActivityKind::parse(&kind).unwrap_or(ActivityKind::Window),
        started_at_ms: row.get(3)?,
        ended_at_ms: row.get(4)?,
        timezone_offset_min: row.get(5)?,
        app_name: row.get(6)?,
        process_name: row.get(7)?,
        window_title: row.get(8)?,
        browser: row.get(9)?,
        domain: row.get(10)?,
        url: row.get(11)?,
        page_title: row.get(12)?,
        interaction_type: row.get(13)?,
        category_id: row.get(14)?,
        project_id: row.get(15)?,
        classification_source: classification
            .as_deref()
            .and_then(ClassificationSource::parse),
        is_afk: row.get::<_, i64>(17)? != 0,
        metadata_json: row.get(18)?,
        created_at_ms: row.get(19)?,
        updated_at_ms: row.get(20)?,
    })
}

/// Insert or replace a segment. Replacement is how an open segment is
/// checkpointed every heartbeat without creating duplicates (spec §29).
pub fn upsert(tx: &Transaction<'_>, segment: &ActivitySegment) -> Result<()> {
    tx.execute(
        "INSERT INTO activity_segments (
            id, source, kind, started_at_ms, ended_at_ms, timezone_offset_min,
            app_name, process_name, window_title, browser, domain, url, page_title,
            interaction_type, category_id, project_id, classification_source, is_afk,
            metadata_json, created_at_ms, updated_at_ms
         ) VALUES (
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
            ?19, ?20, ?21
         )
         ON CONFLICT(id) DO UPDATE SET
            ended_at_ms = excluded.ended_at_ms,
            window_title = excluded.window_title,
            page_title = excluded.page_title,
            url = excluded.url,
            category_id = excluded.category_id,
            project_id = excluded.project_id,
            classification_source = excluded.classification_source,
            is_afk = excluded.is_afk,
            metadata_json = excluded.metadata_json,
            updated_at_ms = excluded.updated_at_ms",
        params![
            segment.id,
            segment.source.as_str(),
            segment.kind.as_str(),
            segment.started_at_ms,
            segment.ended_at_ms,
            segment.timezone_offset_min,
            segment.app_name,
            segment.process_name,
            segment.window_title,
            segment.browser,
            segment.domain,
            segment.url,
            segment.page_title,
            segment.interaction_type,
            segment.category_id,
            segment.project_id,
            segment.classification_source.map(|c| c.as_str()),
            i64::from(segment.is_afk),
            segment.metadata_json,
            segment.created_at_ms,
            segment.updated_at_ms,
        ],
    )?;
    Ok(())
}

pub fn insert_many(tx: &Transaction<'_>, segments: &[ActivitySegment]) -> Result<usize> {
    for segment in segments {
        upsert(tx, segment)?;
    }
    Ok(segments.len())
}

pub fn get(conn: &Connection, id: &str) -> Result<ActivitySegment> {
    let sql = format!("SELECT {COLUMNS} FROM activity_segments WHERE id = ?1");
    conn.query_row(&sql, [id], map_row).map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => StorageError::NotFound(format!("segment {id}")),
        other => StorageError::Sqlite(other),
    })
}

/// Load every segment overlapping a range, ordered by start.
///
/// This is the aggregation entry point: reports only ever query the selected
/// period (spec §123).
pub fn load_range(conn: &Connection, from_ms: i64, to_ms: i64) -> Result<Vec<ActivitySegment>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM activity_segments
         WHERE started_at_ms < ?1 AND ended_at_ms > ?2
         ORDER BY started_at_ms ASC"
    );
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params![to_ms, from_ms], map_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Paginated, filtered activity list (spec §81, §121).
pub fn list(
    conn: &Connection,
    filter: &ActivityFilter,
    page: Page,
    order: SortOrder,
) -> Result<Vec<ActivitySegment>> {
    let compiled = filter.compile(1);
    let page = page.clamped();
    let direction = match order {
        SortOrder::Asc => "ASC",
        SortOrder::Desc => "DESC",
    };
    let limit_index = compiled.params.len() + 1;
    let offset_index = compiled.params.len() + 2;
    let sql = format!(
        "SELECT {COLUMNS} FROM activity_segments
         WHERE {}
         ORDER BY started_at_ms {direction}, id {direction}
         LIMIT ?{limit_index} OFFSET ?{offset_index}",
        compiled.where_sql
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut values = compiled.params;
    values.push(rusqlite::types::Value::Integer(page.limit));
    values.push(rusqlite::types::Value::Integer(page.offset));
    let rows = stmt.query_map(params_from_iter(values), map_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

pub fn count(conn: &Connection, filter: &ActivityFilter) -> Result<i64> {
    let compiled = filter.compile(1);
    let sql = format!(
        "SELECT COUNT(*) FROM activity_segments WHERE {}",
        compiled.where_sql
    );
    let mut stmt = conn.prepare(&sql)?;
    let count = stmt.query_row(params_from_iter(compiled.params), |row| row.get(0))?;
    Ok(count)
}

/// Distinct values used to populate filter dropdowns.
pub fn distinct_values(conn: &Connection, column: &str, limit: i64) -> Result<Vec<String>> {
    let column = match column {
        "app_name" | "process_name" | "browser" | "domain" => column,
        other => {
            return Err(StorageError::Invalid(format!(
                "column {other} is not filterable"
            )))
        }
    };
    let sql = format!(
        "SELECT {column} FROM activity_segments
         WHERE {column} IS NOT NULL AND {column} <> ''
         GROUP BY {column}
         ORDER BY SUM(ended_at_ms - started_at_ms) DESC
         LIMIT ?1"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map([limit], |row| row.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Manual classification override (spec §66, §115).
pub fn set_classification(
    tx: &Transaction<'_>,
    id: &str,
    category_id: Option<&str>,
    project_id: Option<&str>,
    now_ms: i64,
) -> Result<()> {
    let changed = tx.execute(
        "UPDATE activity_segments
         SET category_id = ?2, project_id = ?3, classification_source = 'MANUAL', updated_at_ms = ?4
         WHERE id = ?1",
        params![id, category_id, project_id, now_ms],
    )?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!("segment {id}")));
    }
    Ok(())
}

/// Apply a rule-derived classification, never overwriting a manual override.
pub fn apply_rule_classification(
    tx: &Transaction<'_>,
    id: &str,
    category_id: Option<&str>,
    project_id: Option<&str>,
    now_ms: i64,
) -> Result<bool> {
    let changed = tx.execute(
        "UPDATE activity_segments
         SET category_id = ?2, project_id = ?3, classification_source = 'RULE', updated_at_ms = ?4
         WHERE id = ?1 AND COALESCE(classification_source, '') <> 'MANUAL'",
        params![id, category_id, project_id, now_ms],
    )?;
    Ok(changed > 0)
}

/// Split a segment at a timestamp (spec §116).
pub fn split(
    tx: &Transaction<'_>,
    id: &str,
    at_ms: i64,
    new_id: &str,
    now_ms: i64,
) -> Result<(String, String)> {
    let sql = format!("SELECT {COLUMNS} FROM activity_segments WHERE id = ?1");
    let segment: ActivitySegment = tx.query_row(&sql, [id], map_row).map_err(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => StorageError::NotFound(format!("segment {id}")),
        other => StorageError::Sqlite(other),
    })?;

    if at_ms <= segment.started_at_ms || at_ms >= segment.ended_at_ms {
        return Err(StorageError::Invalid(
            "split point must be strictly inside the segment".into(),
        ));
    }

    let mut tail = segment.clone();
    tail.id = new_id.to_string();
    tail.started_at_ms = at_ms;
    tail.created_at_ms = now_ms;
    tail.updated_at_ms = now_ms;

    tx.execute(
        "UPDATE activity_segments SET ended_at_ms = ?2, updated_at_ms = ?3 WHERE id = ?1",
        params![id, at_ms, now_ms],
    )?;
    upsert(tx, &tail)?;
    Ok((segment.id, tail.id))
}

/// Replace a segment's metadata blob (notes and the local edit audit trail).
pub fn set_metadata(
    tx: &Transaction<'_>,
    id: &str,
    metadata_json: Option<&str>,
    now_ms: i64,
) -> Result<()> {
    let changed = tx.execute(
        "UPDATE activity_segments SET metadata_json = ?2, updated_at_ms = ?3 WHERE id = ?1",
        params![id, metadata_json, now_ms],
    )?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!("segment {id}")));
    }
    Ok(())
}

pub fn delete(tx: &Transaction<'_>, id: &str) -> Result<()> {
    let changed = tx.execute("DELETE FROM activity_segments WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(StorageError::NotFound(format!("segment {id}")));
    }
    Ok(())
}

/// Delete every segment overlapping a range (spec §111).
///
/// Segments straddling the boundary are trimmed rather than deleted whole, so
/// "delete the last 5 minutes" never removes an hour of unrelated activity.
pub fn delete_range(tx: &Transaction<'_>, from_ms: i64, to_ms: i64, now_ms: i64) -> Result<usize> {
    let deleted = tx.execute(
        "DELETE FROM activity_segments
         WHERE started_at_ms >= ?1 AND ended_at_ms <= ?2",
        params![from_ms, to_ms],
    )?;

    // Segment fully containing the range: keep the head, move the tail out.
    let mut straddling: Vec<ActivitySegment> = Vec::new();
    {
        let sql = format!(
            "SELECT {COLUMNS} FROM activity_segments
             WHERE started_at_ms < ?1 AND ended_at_ms > ?2"
        );
        let mut stmt = tx.prepare(&sql)?;
        let rows = stmt.query_map(params![from_ms, to_ms], map_row)?;
        for row in rows {
            straddling.push(row?);
        }
    }
    for segment in straddling {
        tx.execute(
            "UPDATE activity_segments SET ended_at_ms = ?2, updated_at_ms = ?3 WHERE id = ?1",
            params![segment.id, from_ms, now_ms],
        )?;
        let mut tail = segment.clone();
        tail.id = uuid::Uuid::new_v4().to_string();
        tail.started_at_ms = to_ms;
        tail.created_at_ms = now_ms;
        tail.updated_at_ms = now_ms;
        if tail.ended_at_ms > tail.started_at_ms {
            upsert(tx, &tail)?;
        }
    }

    // Overlapping heads and tails are trimmed to the range boundary.
    tx.execute(
        "UPDATE activity_segments
         SET ended_at_ms = ?1, updated_at_ms = ?3
         WHERE started_at_ms < ?1 AND ended_at_ms > ?1 AND ended_at_ms <= ?2",
        params![from_ms, to_ms, now_ms],
    )?;
    tx.execute(
        "UPDATE activity_segments
         SET started_at_ms = ?2, updated_at_ms = ?3
         WHERE started_at_ms >= ?1 AND started_at_ms < ?2 AND ended_at_ms > ?2",
        params![from_ms, to_ms, now_ms],
    )?;

    // Anything left with no duration is noise.
    tx.execute(
        "DELETE FROM activity_segments WHERE ended_at_ms <= started_at_ms",
        [],
    )?;

    Ok(deleted)
}

pub fn delete_all(tx: &Transaction<'_>) -> Result<usize> {
    Ok(tx.execute("DELETE FROM activity_segments", [])?)
}

/// Segments that a rule change may reclassify, excluding manual overrides.
pub fn load_reclassifiable(
    conn: &Connection,
    from_ms: Option<i64>,
    to_ms: Option<i64>,
) -> Result<Vec<ActivitySegment>> {
    let sql = format!(
        "SELECT {COLUMNS} FROM activity_segments
         WHERE COALESCE(classification_source, '') <> 'MANUAL'
           AND (?1 IS NULL OR ended_at_ms > ?1)
           AND (?2 IS NULL OR started_at_ms < ?2)
         ORDER BY started_at_ms ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![from_ms, to_ms], map_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}
