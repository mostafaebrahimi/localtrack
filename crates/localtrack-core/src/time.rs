//! Time helpers.
//!
//! Canonical storage format is **UTC Unix epoch milliseconds** stored as
//! INTEGER (spec §13). Formatted dates are never the canonical timestamp.

use chrono::{DateTime, Datelike, Duration, Local, NaiveDate, TimeZone, Utc};

/// Milliseconds in common units.
pub const SECOND_MS: i64 = 1_000;
pub const MINUTE_MS: i64 = 60 * SECOND_MS;
pub const HOUR_MS: i64 = 60 * MINUTE_MS;
pub const DAY_MS: i64 = 24 * HOUR_MS;

/// Current wall-clock time in UTC epoch milliseconds.
pub fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

/// Current local UTC offset in minutes (east of UTC is positive).
pub fn local_offset_minutes() -> i32 {
    Local::now().offset().local_minus_utc() / 60
}

/// Local UTC offset in minutes at a specific instant.
///
/// This is offset-at-that-instant, so DST transitions are handled correctly and
/// a day is never assumed to be exactly 24 hours (spec §136).
pub fn offset_minutes_at(ts_ms: i64) -> i32 {
    let dt = Utc
        .timestamp_millis_opt(ts_ms)
        .single()
        .unwrap_or_else(Utc::now);
    Local
        .from_utc_datetime(&dt.naive_utc())
        .offset()
        .local_minus_utc()
        / 60
}

/// Start of the local day containing `ts_ms`, expressed in UTC epoch ms.
pub fn local_day_start_ms(ts_ms: i64) -> i64 {
    let dt = Utc
        .timestamp_millis_opt(ts_ms)
        .single()
        .unwrap_or_else(Utc::now);
    let local = Local.from_utc_datetime(&dt.naive_utc());
    let date = local.date_naive();
    local_date_start_ms(date)
}

/// Start of a local calendar date in UTC epoch ms, DST-safe.
pub fn local_date_start_ms(date: NaiveDate) -> i64 {
    // A local midnight can be skipped by DST; take the earliest valid instant.
    match date.and_hms_opt(0, 0, 0) {
        Some(naive) => match Local.from_local_datetime(&naive).earliest() {
            Some(dt) => dt.timestamp_millis(),
            None => {
                // Midnight does not exist on this date (spring-forward);
                // step forward until a valid local time is found.
                let mut probe = naive;
                for _ in 0..(4 * 60) {
                    probe += Duration::minutes(1);
                    if let Some(dt) = Local.from_local_datetime(&probe).earliest() {
                        return dt.timestamp_millis();
                    }
                }
                Utc.from_utc_datetime(&naive).timestamp_millis()
            }
        },
        None => 0,
    }
}

/// Exclusive end of the local day containing `ts_ms` (i.e. next local midnight).
///
/// The result is *not* necessarily `start + 24h`: DST days are 23 or 25 hours.
pub fn local_day_end_ms(ts_ms: i64) -> i64 {
    let start = local_day_start_ms(ts_ms);
    let dt = Utc
        .timestamp_millis_opt(start)
        .single()
        .unwrap_or_else(Utc::now);
    let date = Local.from_utc_datetime(&dt.naive_utc()).date_naive();
    let next = date.succ_opt().unwrap_or(date);
    local_date_start_ms(next)
}

/// Start of the local week (Monday) containing `ts_ms`, in UTC epoch ms.
pub fn local_week_start_ms(ts_ms: i64) -> i64 {
    let day_start = local_day_start_ms(ts_ms);
    let dt = Utc
        .timestamp_millis_opt(day_start)
        .single()
        .unwrap_or_else(Utc::now);
    let local = Local.from_utc_datetime(&dt.naive_utc());
    // Monday is 0; weeks that contain a DST change are still whole weeks.
    let offset = local.weekday().num_days_from_monday() as i64;
    let mut date = local.date_naive();
    for _ in 0..offset {
        date = date.pred_opt().unwrap_or(date);
    }
    local_date_start_ms(date)
}

/// Exclusive end of the local week containing `ts_ms`.
pub fn local_week_end_ms(ts_ms: i64) -> i64 {
    let start = local_week_start_ms(ts_ms);
    let dt = Utc
        .timestamp_millis_opt(start)
        .single()
        .unwrap_or_else(Utc::now);
    let mut date = Local.from_utc_datetime(&dt.naive_utc()).date_naive();
    for _ in 0..7 {
        date = date.succ_opt().unwrap_or(date);
    }
    local_date_start_ms(date)
}

/// Local weeks spanned by a range, as `(label, start, end)`.
///
/// The label is the ISO week, e.g. `2026-W34`, which sorts and reads well.
pub fn local_weeks_in_range(from_ms: i64, to_ms: i64) -> Vec<(String, i64, i64)> {
    let mut out = Vec::new();
    if to_ms <= from_ms {
        return out;
    }
    let mut cursor = local_week_start_ms(from_ms);
    let mut guard = 0;
    while cursor < to_ms && guard < 600 {
        let end = local_week_end_ms(cursor);
        out.push((format_iso_week(cursor), cursor, end));
        cursor = end;
        guard += 1;
    }
    out
}

/// `2026-W34` for the local week containing `ts_ms`.
pub fn format_iso_week(ts_ms: i64) -> String {
    let dt = Utc
        .timestamp_millis_opt(ts_ms)
        .single()
        .unwrap_or_else(Utc::now);
    let local = Local.from_utc_datetime(&dt.naive_utc());
    let iso = local.date_naive().iso_week();
    format!("{}-W{:02}", iso.year(), iso.week())
}

/// Local calendar dates (as `YYYY-MM-DD`) spanned by a UTC range.
pub fn local_days_in_range(from_ms: i64, to_ms: i64) -> Vec<(String, i64, i64)> {
    let mut out = Vec::new();
    if to_ms <= from_ms {
        return out;
    }
    let mut cursor = local_day_start_ms(from_ms);
    let mut guard = 0;
    while cursor < to_ms && guard < 4000 {
        let end = local_day_end_ms(cursor);
        out.push((format_local_date(cursor), cursor, end));
        cursor = end;
        guard += 1;
    }
    out
}

/// Format a UTC timestamp as a local `YYYY-MM-DD` date string.
pub fn format_local_date(ts_ms: i64) -> String {
    let dt = Utc
        .timestamp_millis_opt(ts_ms)
        .single()
        .unwrap_or_else(Utc::now);
    let local = Local.from_utc_datetime(&dt.naive_utc());
    format!(
        "{:04}-{:02}-{:02}",
        local.year(),
        local.month(),
        local.day()
    )
}

/// Format a UTC timestamp as a local `HH:MM:SS` clock string.
pub fn format_local_time(ts_ms: i64) -> String {
    let dt = Utc
        .timestamp_millis_opt(ts_ms)
        .single()
        .unwrap_or_else(Utc::now);
    let local: DateTime<Local> = Local.from_utc_datetime(&dt.naive_utc());
    local.format("%H:%M:%S").to_string()
}

/// Format a duration in ms as `HHh MMm` (spec dashboard style).
pub fn format_duration_hm(ms: i64) -> String {
    let total_minutes = ms.max(0) / MINUTE_MS;
    format!("{:02}h {:02}m", total_minutes / 60, total_minutes % 60)
}

/// Format a duration in ms as `HH:MM:SS`.
pub fn format_duration_hms(ms: i64) -> String {
    let total = ms.max(0) / SECOND_MS;
    format!(
        "{:02}:{:02}:{:02}",
        total / 3600,
        (total % 3600) / 60,
        total % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn day_bounds_are_ordered_and_cover_ts() {
        let now = now_ms();
        let start = local_day_start_ms(now);
        let end = local_day_end_ms(now);
        assert!(start <= now && now < end);
        // A local day is 23, 24 or 25 hours long depending on DST.
        let len = end - start;
        assert!((23 * HOUR_MS..=25 * HOUR_MS).contains(&len), "len={len}");
    }

    #[test]
    fn range_splits_into_days() {
        let start = local_day_start_ms(now_ms());
        let days = local_days_in_range(start, start + 3 * DAY_MS);
        assert!(days.len() == 3 || days.len() == 4);
        assert_eq!(days[0].1, start);
    }

    #[test]
    fn weeks_start_on_monday_and_last_seven_days() {
        let now = now_ms();
        let start = local_week_start_ms(now);
        let end = local_week_end_ms(now);
        assert!(start <= now && now < end);

        let dt = Utc.timestamp_millis_opt(start).single().unwrap();
        let local = Local.from_utc_datetime(&dt.naive_utc());
        assert_eq!(
            local.weekday().num_days_from_monday(),
            0,
            "weeks start on Monday"
        );

        // 7 local days, which is 7×24h except across a DST change.
        let length = end - start;
        assert!((7 * DAY_MS - HOUR_MS..=7 * DAY_MS + HOUR_MS).contains(&length));
    }

    #[test]
    fn a_range_splits_into_whole_weeks() {
        let start = local_week_start_ms(now_ms());
        let weeks = local_weeks_in_range(start, start + 21 * DAY_MS);
        assert_eq!(weeks.len(), 3);
        assert_eq!(weeks[0].1, start);
        assert_eq!(weeks[0].2, weeks[1].1, "weeks are contiguous");
        assert!(weeks[0].0.contains("-W"), "labelled as an ISO week");
    }

    #[test]
    fn formats_durations() {
        assert_eq!(format_duration_hm(8 * HOUR_MS + 34 * MINUTE_MS), "08h 34m");
        assert_eq!(
            format_duration_hms(3 * HOUR_MS + 28 * MINUTE_MS + 14 * SECOND_MS),
            "03:28:14"
        );
        assert_eq!(format_duration_hm(-5), "00h 00m");
    }
}
