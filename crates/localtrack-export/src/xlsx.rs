use std::path::{Path, PathBuf};

use localtrack_core::aggregation::ReportSet;
use localtrack_core::time::{format_local_date, format_local_time, MINUTE_MS};
use rust_xlsxwriter::{Format, FormatAlign, Workbook, Worksheet};

use crate::model::{ActivityExportRow, ExportData, ExportOptions};
use crate::Result;

fn hours(ms: i64) -> f64 {
    // Durations are written as decimal hours so they aggregate in a spreadsheet.
    (ms as f64) / 3_600_000.0
}

fn header_format() -> Format {
    Format::new().set_bold().set_align(FormatAlign::Left)
}

fn duration_format() -> Format {
    Format::new().set_num_format("0.00")
}

fn write_headers(sheet: &mut Worksheet, headers: &[&str]) -> Result<()> {
    let format = header_format();
    for (index, header) in headers.iter().enumerate() {
        sheet.write_string_with_format(0, index as u16, *header, &format)?;
    }
    sheet.set_freeze_panes(1, 0)?;
    Ok(())
}

fn write_report_sheet(workbook: &mut Workbook, title: &str, report: &ReportSet) -> Result<()> {
    let sheet = workbook.add_worksheet();
    sheet.set_name(title)?;
    write_headers(
        sheet,
        &["Name", "Detail", "Duration (h)", "Share (%)", "Visits"],
    )?;
    let duration = duration_format();
    for (index, row) in report.rows.iter().enumerate() {
        let r = index as u32 + 1;
        sheet.write_string(r, 0, &row.label)?;
        sheet.write_string(r, 1, row.secondary.as_deref().unwrap_or(""))?;
        sheet.write_number_with_format(r, 2, hours(row.duration_ms), &duration)?;
        sheet.write_number_with_format(r, 3, row.percentage, &duration)?;
        sheet.write_number(r, 4, row.visit_count as f64)?;
    }
    sheet.autofit();
    Ok(())
}

fn write_activity_sheet(
    workbook: &mut Workbook,
    title: &str,
    rows: &[ActivityExportRow],
    options: &ExportOptions,
) -> Result<()> {
    let sheet = workbook.add_worksheet();
    sheet.set_name(title)?;
    write_headers(
        sheet,
        &[
            "Date",
            "Start",
            "End",
            "Duration (h)",
            "Source",
            "Kind",
            "Application",
            "Process",
            "Window Title",
            "Browser",
            "Domain",
            "Page Title",
            "URL",
            "Category",
            "Project",
            "Interaction Type",
        ],
    )?;
    let duration = duration_format();
    for (index, row) in rows.iter().enumerate() {
        let r = index as u32 + 1;
        let segment = &row.segment;
        sheet.write_string(r, 0, format_local_date(segment.started_at_ms))?;
        sheet.write_string(r, 1, format_local_time(segment.started_at_ms))?;
        sheet.write_string(r, 2, format_local_time(segment.ended_at_ms))?;
        sheet.write_number_with_format(r, 3, hours(segment.duration_ms()), &duration)?;
        sheet.write_string(r, 4, segment.source.as_str())?;
        sheet.write_string(r, 5, segment.kind.as_str())?;
        sheet.write_string(r, 6, segment.app_name.as_deref().unwrap_or(""))?;
        sheet.write_string(r, 7, segment.process_name.as_deref().unwrap_or(""))?;
        sheet.write_string(r, 8, segment.window_title.as_deref().unwrap_or(""))?;
        sheet.write_string(r, 9, segment.browser.as_deref().unwrap_or(""))?;
        sheet.write_string(r, 10, segment.domain.as_deref().unwrap_or(""))?;
        sheet.write_string(r, 11, segment.page_title.as_deref().unwrap_or(""))?;
        sheet.write_string(
            r,
            12,
            options
                .url_privacy
                .apply(segment.url.as_deref())
                .unwrap_or_default(),
        )?;
        sheet.write_string(r, 13, row.category_name.as_deref().unwrap_or(""))?;
        sheet.write_string(r, 14, row.project_name.as_deref().unwrap_or(""))?;
        sheet.write_string(r, 15, segment.interaction_type.as_deref().unwrap_or(""))?;
    }
    sheet.autofit();
    Ok(())
}

/// Write the workbook described in spec §90.
pub fn write_workbook<P: AsRef<Path>>(
    path: P,
    data: &ExportData,
    options: &ExportOptions,
) -> Result<PathBuf> {
    let mut workbook = Workbook::new();
    let duration = duration_format();

    if options.include_summary {
        let sheet = workbook.add_worksheet();
        sheet.set_name("Summary")?;
        write_headers(
            sheet,
            &[
                "Start Date",
                "End Date",
                "Clocked (h)",
                "Work (h)",
                "Active (h)",
                "Idle (h)",
                "Break (h)",
                "Untracked (h)",
                "Context Switches",
                "Average Focus (min)",
                "Longest Focus (min)",
            ],
        )?;
        let s = &data.summary;
        sheet.write_string(1, 0, format_local_date(data.from_ms))?;
        sheet.write_string(1, 1, format_local_date(data.to_ms.saturating_sub(1)))?;
        sheet.write_number_with_format(1, 2, hours(s.clocked_ms), &duration)?;
        sheet.write_number_with_format(1, 3, hours(s.work_ms), &duration)?;
        sheet.write_number_with_format(1, 4, hours(s.active_ms), &duration)?;
        sheet.write_number_with_format(1, 5, hours(s.idle_ms), &duration)?;
        sheet.write_number_with_format(1, 6, hours(s.break_ms), &duration)?;
        sheet.write_number_with_format(1, 7, hours(s.untracked_ms), &duration)?;
        sheet.write_number(1, 8, s.context_switches as f64)?;
        sheet.write_number_with_format(
            1,
            9,
            s.average_focus_ms as f64 / MINUTE_MS as f64,
            &duration,
        )?;
        sheet.write_number_with_format(
            1,
            10,
            s.longest_focus_ms as f64 / MINUTE_MS as f64,
            &duration,
        )?;
        sheet.autofit();
    }

    if options.include_daily_summary {
        let sheet = workbook.add_worksheet();
        sheet.set_name("Daily Summary")?;
        write_headers(
            sheet,
            &[
                "Date",
                "First Clock In",
                "Last Clock Out",
                "Clocked (h)",
                "Work (h)",
                "Active (h)",
                "Idle (h)",
                "Break (h)",
                "Untracked (h)",
            ],
        )?;
        for (index, day) in data.daily.iter().enumerate() {
            let r = index as u32 + 1;
            sheet.write_string(r, 0, &day.date)?;
            sheet.write_string(
                r,
                1,
                day.first_clock_in_ms
                    .map(format_local_time)
                    .unwrap_or_default(),
            )?;
            sheet.write_string(
                r,
                2,
                day.last_clock_out_ms
                    .map(format_local_time)
                    .unwrap_or_default(),
            )?;
            sheet.write_number_with_format(r, 3, hours(day.summary.clocked_ms), &duration)?;
            sheet.write_number_with_format(r, 4, hours(day.summary.work_ms), &duration)?;
            sheet.write_number_with_format(r, 5, hours(day.summary.active_ms), &duration)?;
            sheet.write_number_with_format(r, 6, hours(day.summary.idle_ms), &duration)?;
            sheet.write_number_with_format(r, 7, hours(day.summary.break_ms), &duration)?;
            sheet.write_number_with_format(r, 8, hours(day.summary.untracked_ms), &duration)?;
        }
        sheet.autofit();
    }

    if options.include_sessions {
        let sheet = workbook.add_worksheet();
        sheet.set_name("Sessions")?;
        write_headers(
            sheet,
            &[
                "Date",
                "Clock In",
                "Clock Out",
                "Clocked (h)",
                "Break (h)",
                "Work (h)",
                "Active (h)",
                "Idle (h)",
                "Untracked (h)",
                "Note",
            ],
        )?;
        for (index, session) in data.sessions.iter().enumerate() {
            let r = index as u32 + 1;
            sheet.write_string(r, 0, &session.date)?;
            sheet.write_string(r, 1, format_local_time(session.clock_in_ms))?;
            sheet.write_string(
                r,
                2,
                session
                    .clock_out_ms
                    .map(format_local_time)
                    .unwrap_or_default(),
            )?;
            sheet.write_number_with_format(r, 3, hours(session.clocked_ms), &duration)?;
            sheet.write_number_with_format(r, 4, hours(session.break_ms), &duration)?;
            sheet.write_number_with_format(r, 5, hours(session.work_ms), &duration)?;
            sheet.write_number_with_format(r, 6, hours(session.active_ms), &duration)?;
            sheet.write_number_with_format(r, 7, hours(session.idle_ms), &duration)?;
            sheet.write_number_with_format(r, 8, hours(session.untracked_ms), &duration)?;
            sheet.write_string(r, 9, session.note.as_deref().unwrap_or(""))?;
        }
        sheet.autofit();
    }

    if options.include_applications {
        if let Some(report) = &data.applications {
            write_report_sheet(&mut workbook, "Applications", report)?;
        }
    }
    if options.include_websites {
        if let Some(report) = &data.websites {
            write_report_sheet(&mut workbook, "Websites", report)?;
        }
    }
    if options.include_pages {
        if let Some(report) = &data.pages {
            write_report_sheet(&mut workbook, "Pages", report)?;
        }
    }
    if options.include_categories {
        if let Some(report) = &data.categories {
            write_report_sheet(&mut workbook, "Categories", report)?;
        }
    }
    if options.include_projects {
        if let Some(report) = &data.projects {
            write_report_sheet(&mut workbook, "Projects", report)?;
        }
    }

    if options.include_activity_detail {
        write_activity_sheet(&mut workbook, "Activity Detail", &data.activities, options)?;
    }

    // The Interactions sheet only exists when detailed tracking produced data.
    if options.include_interactions && !data.interactions.is_empty() {
        write_activity_sheet(&mut workbook, "Interactions", &data.interactions, options)?;
    }

    let path = path.as_ref().to_path_buf();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    workbook.save(&path)?;
    Ok(path)
}
