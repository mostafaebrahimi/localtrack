//! Reading a window title for the work behind it.
//!
//! A day's tracking is a list of applications — "Code, 1h 28m; Gnome Terminal,
//! 1h 7m" — which says nothing about *what* was being worked on. The titles
//! those applications put in their window bar do: the editor names the project
//! folder, the terminal names the directory or the task, the browser names the
//! repository or the ticket.
//!
//! This module turns a title into a [`WorkContext`]: a workspace (the thing
//! being worked on) and a detail (the file, channel or path inside it). It is a
//! pure function of what was already recorded, so it reads history as well as
//! new activity, and nothing extra is collected to make it work.

use serde::{Deserialize, Serialize};

use crate::activity::ActivitySegment;

/// Where a workspace name was read from.
///
/// The kind is what makes names comparable: "helpdesk-v2" the editor project
/// and "helpdesk-v2" the GitLab page are the same work seen through two
/// windows, and both are worth more than a title nobody can place.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ContextKind {
    /// An editor or IDE with a project open.
    Editor,
    /// A terminal, named after its directory, its host or its task.
    Terminal,
    /// A page whose title names a repository, ticket or document.
    Browser,
    /// A conversation.
    Chat,
    /// A document in an office or notes application.
    Document,
    /// Nothing recognisable.
    #[default]
    Unknown,
}

impl ContextKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ContextKind::Editor => "EDITOR",
            ContextKind::Terminal => "TERMINAL",
            ContextKind::Browser => "BROWSER",
            ContextKind::Chat => "CHAT",
            ContextKind::Document => "DOCUMENT",
            ContextKind::Unknown => "UNKNOWN",
        }
    }
}

/// What a window title says about the work behind it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkContext {
    /// The project, repository, directory, task or conversation.
    pub workspace: Option<String>,
    /// What was open inside it: a file, a page, a path.
    pub detail: Option<String>,
    pub kind: ContextKind,
}

impl WorkContext {
    fn of(kind: ContextKind, workspace: Option<&str>, detail: Option<&str>) -> Self {
        let workspace = workspace.and_then(sane);
        // A context with nothing to say is no context at all.
        if workspace.is_none() {
            return WorkContext::default();
        }
        WorkContext {
            workspace,
            detail: detail.and_then(sane),
            kind,
        }
    }

    /// True when the title yielded nothing worth reporting.
    pub fn is_empty(&self) -> bool {
        self.workspace.is_none()
    }
}

/// A workspace name has to be short enough to read in a report.
const MAX_LEN: usize = 90;

/// Titles that name nothing.
///
/// A browser tab between pages, or a window that has not decided what it holds
/// yet, would otherwise become a "project" — and, in employee mode, a rule on
/// the server about a project called "New Tab".
const MEANINGLESS: &[&str] = &[
    "untitled",
    "new tab",
    "about:blank",
    "blank page",
    "loading",
    "loading…",
    "no title",
    "start page",
    "home page",
];

/// Trim a candidate name, discarding what is empty or absurdly long.
fn sane(value: &str) -> Option<String> {
    let trimmed = value
        .trim()
        .trim_matches(|c: char| matches!(c, '-' | '–' | '—' | '·' | '|' | ':' | '*'))
        .trim();
    if trimmed.is_empty() || trimmed.chars().count() > MAX_LEN {
        return None;
    }
    if MEANINGLESS.contains(&trimmed.to_ascii_lowercase().as_str()) {
        return None;
    }
    Some(trimmed.to_string())
}

/// Strip the decoration applications put around their titles.
///
/// Progress spinners, notification counts and the invisible bidirectional marks
/// that chat applications wrap names in are all noise, and leaving them in would
/// make one workspace look like several.
pub fn clean_title(title: &str) -> String {
    let without_marks: String = title
        .chars()
        .filter(|c| {
            !matches!(*c,
                '\u{200e}' | '\u{200f}'                 // left/right-to-left marks
                | '\u{2066}'..='\u{2069}'               // isolates
                | '\u{202a}'..='\u{202e}'               // embeddings and overrides
            )
        })
        .collect();

    let is_spinner = |c: char| {
        matches!(c,
            '✳' | '✶' | '✻' | '✽' | '✢' | '·' | '*'
            | '\u{25d0}'..='\u{25d3}'                   // half-filled circles
            | '\u{2800}'..='\u{28ff}'                   // braille spinners
            | '\u{25cf}' | '\u{25cb}' | '\u{2022}' | '\u{25b6}' | '\u{23f8}' | '\u{2016}'
        )
    };

    without_marks
        .trim_start_matches(|c: char| is_spinner(c) || c.is_whitespace())
        .trim()
        .to_string()
}

/// Read the work context out of a recorded segment.
pub fn work_context(segment: &ActivitySegment) -> WorkContext {
    parse_context(
        segment.app_name.as_deref(),
        segment.process_name.as_deref(),
        segment.window_title.as_deref(),
        segment.domain.as_deref(),
        segment.page_title.as_deref(),
    )
}

/// Read the work context out of the fields a segment carries.
///
/// Browser pages are read from their own title and domain; everything else from
/// the window title, dispatched on which application owns the window.
pub fn parse_context(
    app_name: Option<&str>,
    process_name: Option<&str>,
    window_title: Option<&str>,
    domain: Option<&str>,
    page_title: Option<&str>,
) -> WorkContext {
    let app = app_name.unwrap_or_default().to_ascii_lowercase();
    let process = process_name.unwrap_or_default().to_ascii_lowercase();
    let family = Family::of(&app, &process);

    // A browser page knows its domain, which is a better guide than the window
    // title — and a browser window's title is the page's anyway.
    if let Some(domain) = domain {
        let title = page_title.or(window_title).unwrap_or_default();
        return browser_context(&clean_title(title), domain);
    }

    let title = clean_title(window_title.unwrap_or_default());
    if title.is_empty() {
        return WorkContext::default();
    }

    match family {
        Family::Editor => editor_context(&title),
        Family::Terminal => terminal_context(&title),
        Family::Browser => {
            // A browser window with no page record: its title still ends in the
            // browser's name, and the rest may name a repository or a ticket.
            let (page, _) = split_suffix(&title);
            browser_context(page, "")
        }
        Family::Chat => chat_context(&title, &app),
        Family::Document => document_context(&title, &app),
        Family::Other => WorkContext::default(),
    }
}

/// The families of application whose titles are worth reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Family {
    Editor,
    Terminal,
    Browser,
    Chat,
    Document,
    Other,
}

impl Family {
    fn of(app: &str, process: &str) -> Family {
        const EDITORS: &[&str] = &[
            "code",
            "vscodium",
            "code - oss",
            "cursor",
            "windsurf",
            "visual studio code",
            "intellij",
            "idea",
            "pycharm",
            "webstorm",
            "goland",
            "phpstorm",
            "rubymine",
            "clion",
            "rider",
            "datagrip",
            "android studio",
            "jetbrains",
            "sublime_text",
            "sublime text",
            "zed",
            "nvim",
            "neovim",
            "vim",
            "emacs",
            "gnome-builder",
            "kate",
            "geany",
            "eclipse",
            "netbeans",
            "xcode",
            "fleet",
        ];
        const TERMINALS: &[&str] = &[
            "gnome-terminal",
            "gnome terminal",
            "gnome-terminal-server",
            "konsole",
            "alacritty",
            "kitty",
            "wezterm",
            "wezterm-gui",
            "xterm",
            "urxvt",
            "terminator",
            "tilix",
            "ptyxis",
            "foot",
            "st",
            "hyper",
            "warp",
            "iterm2",
            "terminal",
            "windowsterminal",
            "console",
            "guake",
            "yakuake",
            "deepin-terminal",
        ];
        const BROWSERS: &[&str] = &[
            "chrome",
            "google chrome",
            "chromium",
            "firefox",
            "librewolf",
            "waterfox",
            "zen",
            "navigator",
            "msedge",
            "microsoft edge",
            "brave",
            "brave-browser",
            "vivaldi",
            "opera",
            "safari",
            "epiphany",
        ];
        const CHAT: &[&str] = &[
            "slack",
            "teams",
            "microsoft teams",
            "discord",
            "telegram",
            "telegram desktop",
            "whatsapp",
            "signal",
            "element",
            "rocket.chat",
            "mattermost",
            "zoom",
            "skype",
            "thunderbird",
            "evolution",
            "geary",
            "mail",
        ];
        const DOCUMENTS: &[&str] = &[
            "obsidian",
            "notion",
            "logseq",
            "joplin",
            "libreoffice",
            "soffice",
            "writer",
            "calc",
            "impress",
            "winword",
            "excel",
            "powerpoint",
            "onenote",
            "evince",
            "okular",
            "zathura",
            "acroread",
            "typora",
            "marktext",
            "anytype",
        ];

        for (list, family) in [
            (EDITORS, Family::Editor),
            (TERMINALS, Family::Terminal),
            (BROWSERS, Family::Browser),
            (CHAT, Family::Chat),
            (DOCUMENTS, Family::Document),
        ] {
            if list
                .iter()
                .any(|name| matches_name(app, name) || matches_name(process, name))
            {
                return family;
            }
        }
        Family::Other
    }
}

/// Application names arrive inconsistently — "Code", "code", "Gnome Terminal",
/// "gnome-terminal-server", and X11 sometimes repeats the class ("Firefox
/// Firefox") — so a name matches on whole words rather than as a substring.
fn matches_name(value: &str, name: &str) -> bool {
    if value == name {
        return true;
    }
    value
        .split(|c: char| !c.is_alphanumeric() && c != '.')
        .any(|word| word == name)
        || value.starts_with(&format!("{name} "))
        || value.ends_with(&format!(" {name}"))
}

/// Editors put the project between the file and their own name.
///
/// VS Code and its forks: `file.ts - project - Visual Studio Code`, or
/// `project - Visual Studio Code` with no file open. JetBrains IDEs use
/// `project – file.java`. Sublime uses `file (project) - Sublime Text`.
fn editor_context(title: &str) -> WorkContext {
    let (body, _) = split_suffix(title);

    // Sublime Text: the project is in brackets after the file.
    if let Some(open) = body.find(" (") {
        if let Some(close) = body[open..].find(')') {
            let project = &body[open + 2..open + close];
            if !project.is_empty() {
                return WorkContext::of(ContextKind::Editor, Some(project), Some(&body[..open]));
            }
        }
    }

    let parts: Vec<&str> = split_parts(body);
    match parts.len() {
        0 => WorkContext::default(),
        // "project" — nothing open, or the IDE only names the project.
        1 => WorkContext::of(ContextKind::Editor, Some(parts[0]), None),
        // "file - project" (VS Code) or "project – file" (JetBrains): the part
        // that looks like a file name is the detail, the other is the project.
        _ => {
            let first = parts[0];
            let last = parts[parts.len() - 1];
            if looks_like_file(first) && !looks_like_file(last) {
                WorkContext::of(ContextKind::Editor, Some(last), Some(first))
            } else if looks_like_file(last) && !looks_like_file(first) {
                WorkContext::of(ContextKind::Editor, Some(first), Some(last))
            } else {
                // Neither looks like a file: VS Code's order wins, because the
                // window ends with what owns the window.
                WorkContext::of(ContextKind::Editor, Some(last), Some(first))
            }
        }
    }
}

/// Terminals name a directory, a host or the task running in them.
///
/// `user@host: ~/src/app` and `~/src/app` both mean the directory; a title with
/// no path in it is whatever the shell or the tool set — a tmux session name or
/// the task someone is working on — and that is worth keeping as it stands.
fn terminal_context(title: &str) -> WorkContext {
    // tmux: "[session] 0:zsh*"
    if let Some(rest) = title.strip_prefix('[') {
        if let Some(end) = rest.find(']') {
            let session = &rest[..end];
            return WorkContext::of(
                ContextKind::Terminal,
                Some(session),
                Some(rest[end + 1..].trim()),
            );
        }
    }

    // "user@host: ~/path", and the spaceless "user@host:/path" that some
    // shells write — either way the directory is what comes after the colon.
    let path_part = match title.split_once(':') {
        Some((prompt, path)) if prompt.contains('@') || looks_like_path(path.trim()) => {
            Some(path.trim())
        }
        _ if looks_like_path(title) => Some(title),
        _ => None,
    };

    if let Some(path) = path_part {
        // The last segment of the path is the project; "~" is home itself.
        let cleaned = path.split_whitespace().next().unwrap_or(path);
        let name = cleaned.rsplit('/').find(|part| !part.is_empty());
        return match name {
            Some("~") | None => WorkContext::of(ContextKind::Terminal, Some("Home"), Some(cleaned)),
            Some(name) => WorkContext::of(ContextKind::Terminal, Some(name), Some(cleaned)),
        };
    }

    // No path: the title is a task or session name, which is what task runners and CLI agents
    // put there, and is exactly the label someone would choose.
    WorkContext::of(ContextKind::Terminal, Some(title), None)
}

/// Web pages name their project in ways worth recognising.
///
/// GitHub: `owner/repo: description`, or `Title · Issue #12 · owner/repo`.
/// GitLab: `group / subgroup / project · GitLab`. Jira: `[KEY-12] summary`.
/// Everything else falls back to the site itself, which still separates
/// "documentation" from "the ticket tracker".
fn browser_context(page_title: &str, domain: &str) -> WorkContext {
    let (title, _) = split_suffix(page_title);
    let host = domain.trim_start_matches("www.");

    if title.is_empty() {
        return WorkContext::of(ContextKind::Browser, sane(host).as_deref(), None);
    }

    // GitHub and GitLab both put the project path in the title, separated by
    // a middle dot from whatever page of it is open. The last such path is the
    // repository — the earlier ones are branches and files.
    //
    // This runs whatever the host is, because a browser tracked without the
    // extension has a title and no domain at all.
    let parts = split_parts(title);
    if let Some(project) = parts.iter().rev().find_map(|part| repo_name(part)) {
        return WorkContext::of(ContextKind::Browser, Some(&project), Some(title));
    }

    // Jira and Linear lead with the issue key: "[ABC-12] Summary", "ABC-12 Summary".
    if let Some(key) = issue_key(title) {
        return WorkContext::of(ContextKind::Browser, Some(&key), Some(title));
    }

    // Google Docs and Office name the document first, the product last.
    if host.contains("docs.google") || host.contains("office") || host.contains("sharepoint") {
        if let Some(first) = parts.first() {
            return WorkContext::of(ContextKind::Browser, Some(first), Some(title));
        }
    }

    // Anything else: the site is the closest thing to a project, and the page
    // title is kept as the detail so the reports still see it.
    WorkContext::of(
        ContextKind::Browser,
        sane(host).or_else(|| sane(title)).as_deref(),
        Some(title),
    )
}

/// A repository path such as `owner/repo` or `group / sub / project`.
fn repo_name(part: &str) -> Option<String> {
    // "owner/repo: what the project is" — the description is not the name.
    let trimmed = part.split_once(": ").map_or(part, |(head, _)| head).trim();

    // A repository path is segments of name characters and nothing else, so a
    // page title that merely contains a slash ("12/25 done") is not one.
    let segments: Vec<&str> = trimmed.split('/').map(str::trim).collect();
    if segments.len() < 2 || segments.len() > 4 {
        return None;
    }
    // GitLab writes its groups with spaces around the slashes ("k8s / Helpdesk
    // / HelpDesk V2"), and only then may a segment be more than one word.
    // Without those spaces this has to look like "owner/repo", which keeps
    // ordinary prose such as "12/25 items packed" from passing for a project.
    let spaced = trimmed.contains(" / ");
    if segments.iter().any(|segment| {
        segment.is_empty()
            || !segment
                .chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '.' | '_' | '-' | ' '))
            || (!spaced && segment.contains(' '))
            || segment.split_whitespace().count() > 3
    }) {
        return None;
    }
    sane(segments[segments.len() - 1])
}

/// The `ABC-123` an issue tracker leads with.
fn issue_key(title: &str) -> Option<String> {
    let first = title
        .trim_start_matches('[')
        .split(|c: char| c == ']' || c.is_whitespace())
        .next()?;
    let (project, number) = first.split_once('-')?;
    let project_ok = project.len() >= 2
        && project.len() <= 10
        && project
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit());
    let number_ok = !number.is_empty() && number.chars().all(|c| c.is_ascii_digit());
    (project_ok && number_ok).then(|| project.to_string())
}

/// Strip a leading "(12)" notification count from a title.
///
/// It is a count of unread messages, not part of the name, and it changes as
/// they arrive: left in, the same conversation is a new workspace every time
/// someone writes.
fn strip_leading_count(title: &str) -> &str {
    let trimmed = title.trim_start();
    let Some(rest) = trimmed.strip_prefix('(') else {
        return trimmed;
    };
    let Some((digits, after)) = rest.split_once(')') else {
        return trimmed;
    };
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return trimmed;
    }
    let after = after.trim_start();
    // "(3)" on its own is a count and nothing else; keep the original so the
    // caller's own emptiness checks decide what that means.
    if after.is_empty() {
        return trimmed;
    }
    after
}

/// Chat applications name the conversation, and often little else.
fn chat_context(title: &str, app: &str) -> WorkContext {
    // Slack: "workspace | #channel" or "#channel - workspace - Slack".
    if app.contains("slack") {
        let parts = split_pipe(title);
        if parts.len() >= 2 {
            return WorkContext::of(ContextKind::Chat, Some(parts[0]), Some(parts[1]));
        }
    }

    let (body, _) = split_suffix(title);
    // Telegram puts the global unread count in front of the conversation name
    // and that conversation's own count behind it — "(1) Kh .Z Jamali – (2)".
    // The trailing one leaves with the suffix; the leading one has to go too,
    // or one conversation becomes a row per unread state it passed through.
    let body = strip_leading_count(body);
    // "Telegram (4)" and friends: the unread count is not a conversation.
    let without_count = body
        .trim_end_matches(|c: char| c.is_ascii_digit() || matches!(c, '(' | ')' | '–' | '-' | ' '));
    let name = if without_count.trim().is_empty() {
        body
    } else {
        without_count
    };

    // The application's own name means no conversation is open, whether the
    // window says "Telegram" and the class says "telegram desktop" or vice versa.
    let name = name.trim();
    let lowered = name.to_ascii_lowercase();
    if name.is_empty() || app.contains(&lowered) || lowered.contains(app.trim()) {
        return WorkContext::default();
    }
    WorkContext::of(ContextKind::Chat, Some(name), None)
}

/// Documents name the file, and usually the application after it.
fn document_context(title: &str, app: &str) -> WorkContext {
    let (body, _) = split_suffix(title);
    if body.trim().eq_ignore_ascii_case(app.trim()) {
        return WorkContext::default();
    }
    let parts = split_parts(body);
    match parts.as_slice() {
        // Obsidian: "note - vault - Obsidian"; the vault is the workspace.
        [file, vault, ..] if parts.len() >= 2 => {
            WorkContext::of(ContextKind::Document, Some(vault), Some(file))
        }
        [only] => WorkContext::of(ContextKind::Document, Some(only), None),
        _ => WorkContext::default(),
    }
}

/// Drop the trailing " - Application Name" a window title usually carries.
///
/// Returns the body and the application name, if there was one.
fn split_suffix(title: &str) -> (&str, Option<&str>) {
    const SUFFIXES: &[&str] = &[
        " - Visual Studio Code",
        " - Code - OSS",
        " - VSCodium",
        " - Cursor",
        " - Windsurf",
        " - Google Chrome",
        " — Google Chrome",
        " - Chromium",
        " — Mozilla Firefox",
        " - Mozilla Firefox",
        " — LibreWolf",
        // Edge writes its own name with a zero-width space in it.
        " - Microsoft\u{200b} Edge",
        " - Microsoft Edge",
        " - Brave",
        " - Vivaldi",
        " - Opera",
        " - Sublime Text",
        " - Zed",
        " - Obsidian",
        " - Slack",
        " - Discord",
        " - Telegram",
        " - LibreOffice Writer",
        " - LibreOffice Calc",
        " - LibreOffice Impress",
        " - Microsoft Word",
        " - Microsoft Excel",
        " - Zen Browser",
        " — Zen Browser",
    ];
    for suffix in SUFFIXES {
        let name = suffix.trim_start_matches([' ', '-', '\u{2014}']).trim();
        if let Some(body) = title.strip_suffix(suffix) {
            return (body.trim(), Some(name));
        }
        // A window with nothing open is titled after the application alone.
        if title.trim().eq_ignore_ascii_case(name) {
            return ("", Some(name));
        }
    }
    (title, None)
}

/// Split on the separators window titles use between their parts.
fn split_parts(title: &str) -> Vec<&str> {
    title
        .split(['·', '\u{2013}', '\u{2014}'])
        .flat_map(|part| part.split(" - "))
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

fn split_pipe(title: &str) -> Vec<&str> {
    title
        .split('|')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect()
}

/// Does this part look like a file rather than a project?
fn looks_like_file(part: &str) -> bool {
    let name = part.trim().trim_start_matches(['●', '•', '*']).trim();
    // A dotted name with a short extension, and no spaces: "main.rs", ".gitlab-ci.yml".
    match name.rsplit_once('.') {
        Some((stem, extension)) => {
            !stem.is_empty()
                && !extension.is_empty()
                && extension.len() <= 12
                && !extension.contains(' ')
                && !name.contains(' ')
        }
        None => false,
    }
}

/// Does this look like a filesystem path?
fn looks_like_path(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.starts_with('~') || trimmed.starts_with('/') || trimmed.starts_with("./")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Titles taken from a real day of tracking on a developer's machine.
    fn ctx(app: &str, title: &str) -> WorkContext {
        parse_context(Some(app), None, Some(title), None, None)
    }

    fn page(domain: &str, title: &str) -> WorkContext {
        parse_context(
            Some("Google Chrome"),
            None,
            Some(title),
            Some(domain),
            Some(title),
        )
    }

    #[test]
    fn vs_code_names_the_project_and_the_file() {
        let context = ctx("Code", ".gitlab-ci.yml - helpdesk-v2 - Visual Studio Code");
        assert_eq!(context.workspace.as_deref(), Some("helpdesk-v2"));
        assert_eq!(context.detail.as_deref(), Some(".gitlab-ci.yml"));
        assert_eq!(context.kind, ContextKind::Editor);
    }

    #[test]
    fn vs_code_with_no_file_open_still_names_the_project() {
        let context = ctx("Code", "helpdesk-v2 - Visual Studio Code");
        assert_eq!(context.workspace.as_deref(), Some("helpdesk-v2"));
        assert_eq!(context.detail, None);
    }

    #[test]
    fn an_editor_with_nothing_open_says_nothing() {
        assert!(ctx("Code", "Visual Studio Code").is_empty());
    }

    #[test]
    fn a_modified_file_marker_does_not_become_part_of_the_name() {
        let context = ctx("Code", "● service.rs - atlas - Visual Studio Code");
        assert_eq!(context.workspace.as_deref(), Some("atlas"));
    }

    #[test]
    fn jetbrains_puts_the_project_first() {
        let context = ctx("IntelliJ IDEA", "helpdesk-v2 – OrderService.java");
        assert_eq!(context.workspace.as_deref(), Some("helpdesk-v2"));
        assert_eq!(context.detail.as_deref(), Some("OrderService.java"));
    }

    #[test]
    fn sublime_puts_the_project_in_brackets() {
        let context = ctx("sublime_text", "main.py (analytics) - Sublime Text");
        assert_eq!(context.workspace.as_deref(), Some("analytics"));
        assert_eq!(context.detail.as_deref(), Some("main.py"));
    }

    #[test]
    fn a_terminal_in_a_directory_is_named_after_it() {
        let context = ctx("Gnome Terminal", "user@host: ~/projects/atlas");
        assert_eq!(context.workspace.as_deref(), Some("atlas"));
        assert_eq!(context.detail.as_deref(), Some("~/projects/atlas"));
        assert_eq!(context.kind, ContextKind::Terminal);
    }

    #[test]
    fn a_bare_path_works_too() {
        assert_eq!(
            ctx("Alacritty", "/srv/projects/atlas").workspace.as_deref(),
            Some("atlas")
        );
    }

    #[test]
    fn home_is_named_rather_than_left_blank() {
        assert_eq!(
            ctx("konsole", "user@host: ~").workspace.as_deref(),
            Some("Home")
        );
    }

    #[test]
    fn a_tmux_session_is_the_workspace() {
        let context = ctx("kitty", "[deploy] 0:zsh*");
        assert_eq!(context.workspace.as_deref(), Some("deploy"));
    }

    #[test]
    fn a_terminal_task_name_survives_its_spinner() {
        // CLI agents and task runners put a progress spinner in front of the task.
        for title in [
            "✳ Kubernetes config review",
            "◐ Kubernetes config review",
            "◑ Kubernetes config review",
        ] {
            let context = ctx("Gnome Terminal", title);
            assert_eq!(
                context.workspace.as_deref(),
                Some("Kubernetes config review"),
                "{title}"
            );
        }
    }

    #[test]
    fn a_gitlab_page_is_named_after_the_project() {
        let context = page(
            "gitlab.com",
            "k8s / Helpdesk / HelpDesk V2 · GitLab - Google Chrome",
        );
        assert_eq!(context.workspace.as_deref(), Some("HelpDesk V2"));
        assert_eq!(context.kind, ContextKind::Browser);
    }

    #[test]
    fn a_github_repository_is_the_workspace() {
        let context = page(
            "github.com",
            "rust-lang/cargo: The Rust package manager - Google Chrome",
        );
        assert_eq!(context.workspace.as_deref(), Some("cargo"));
    }

    #[test]
    fn an_issue_key_is_the_workspace() {
        let context = page(
            "company.atlassian.net",
            "[HELP-482] Seed unit rounding - Jira",
        );
        assert_eq!(context.workspace.as_deref(), Some("HELP"));
    }

    #[test]
    fn an_ordinary_page_falls_back_to_its_site() {
        let context = page("console.min.io", "MinIO Console - Google Chrome");
        assert_eq!(context.workspace.as_deref(), Some("console.min.io"));
        assert_eq!(context.detail.as_deref(), Some("MinIO Console"));
    }

    #[test]
    fn slack_names_the_workspace_and_the_channel() {
        let context = ctx("Slack", "Acme Corp | #platform");
        assert_eq!(context.workspace.as_deref(), Some("Acme Corp"));
        assert_eq!(context.detail.as_deref(), Some("#platform"));
        assert_eq!(context.kind, ContextKind::Chat);
    }

    #[test]
    fn a_chat_with_no_conversation_open_says_nothing() {
        assert!(ctx("Telegram Desktop", "Telegram").is_empty());
        assert!(ctx("Telegram Desktop", "Telegram (4)").is_empty());
    }

    /// Telegram brackets the name with counts: the whole application's unread
    /// count in front, this conversation's behind. Both change constantly, and
    /// both used to split one conversation into a row per state.
    #[test]
    fn a_conversation_survives_an_unread_count_on_either_side() {
        let name = |title: &str| ctx("Telegram Desktop", title).workspace;

        assert_eq!(
            name("(1) \u{200e}\u{2068}Kh .Z Jamali\u{2069} \u{2013} (2)").as_deref(),
            Some("Kh .Z Jamali")
        );
        // The same conversation at a different unread state is the same row.
        assert_eq!(
            name("(1) \u{200e}\u{2068}Kh .Z Jamali\u{2069} \u{2013} (4)"),
            name("\u{200e}\u{2068}Kh .Z Jamali\u{2069}")
        );
        // A name that merely starts with a bracketed word is not a count.
        assert_eq!(
            name("(draft) Kh .Z Jamali").as_deref(),
            Some("(draft) Kh .Z Jamali")
        );
    }

    #[test]
    fn a_conversation_keeps_its_name_without_the_unread_count() {
        // Chat applications wrap names in bidirectional isolate marks.
        let context = ctx(
            "Telegram Desktop",
            "\u{200e}\u{2068}Kh .Z Jamali\u{2069} – (3)",
        );
        assert_eq!(context.workspace.as_deref(), Some("Kh .Z Jamali"));
    }

    #[test]
    fn obsidian_names_the_vault() {
        let context = ctx("obsidian", "Weekly review - Work - Obsidian");
        assert_eq!(context.workspace.as_deref(), Some("Work"));
        assert_eq!(context.detail.as_deref(), Some("Weekly review"));
        assert_eq!(context.kind, ContextKind::Document);
    }

    #[test]
    fn an_unrecognised_application_says_nothing() {
        assert!(ctx("Flameshot", "flameshot").is_empty());
        assert!(ctx("Freelens", "Freelens").is_empty());
    }

    #[test]
    fn an_empty_title_says_nothing() {
        assert!(ctx("Code", "").is_empty());
        assert!(parse_context(None, None, None, None, None).is_empty());
    }

    #[test]
    fn a_repeated_window_class_still_matches_its_family() {
        // X11 hands over "Firefox Firefox" for some windows.
        let context = parse_context(
            Some("Firefox Firefox"),
            None,
            Some("MinIO Console — Mozilla Firefox"),
            None,
            None,
        );
        assert_eq!(context.kind, ContextKind::Browser);
        assert_eq!(context.workspace.as_deref(), Some("MinIO Console"));
    }

    #[test]
    fn a_prompt_without_a_space_is_still_a_prompt() {
        let context = ctx("Gnome Terminal", "user@workstation:/srv/work/pulse");
        assert_eq!(context.workspace.as_deref(), Some("pulse"));
        assert_eq!(context.kind, ContextKind::Terminal);
    }

    #[test]
    fn a_repository_is_found_even_with_no_domain_recorded() {
        // A browser tracked without the extension has a title and nothing else.
        let context = ctx(
            "Google Chrome",
            "Comparing main...feature/search-index · example-org/storefront-api - Google Chrome",
        );
        assert_eq!(context.workspace.as_deref(), Some("storefront-api"));
    }

    #[test]
    fn a_slash_in_a_sentence_is_not_a_repository() {
        let context = page("shop.example.com", "12/25 items packed - Google Chrome");
        assert_eq!(context.workspace.as_deref(), Some("shop.example.com"));
    }

    #[test]
    fn a_tab_between_pages_is_not_a_project() {
        assert!(ctx("Google Chrome", "New Tab - Google Chrome").is_empty());
        assert!(ctx("Google Chrome", "Untitled - Google Chrome").is_empty());
        // The domain still counts when the extension recorded one.
        assert_eq!(
            page("example.com", "Untitled - Google Chrome")
                .workspace
                .as_deref(),
            Some("example.com")
        );
    }

    #[test]
    fn an_absurdly_long_title_is_not_a_workspace() {
        let long = "x".repeat(200);
        assert!(ctx("Gnome Terminal", &long).is_empty());
    }

    #[test]
    fn cleaning_a_title_leaves_ordinary_text_alone() {
        assert_eq!(
            clean_title("helpdesk-v2 - Visual Studio Code"),
            "helpdesk-v2 - Visual Studio Code"
        );
        assert_eq!(clean_title("  ✳  Task  "), "Task");
    }
}
