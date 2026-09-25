//! What the title reader makes of a real database.
//!
//! Usage: cargo run -p localtrack-core --example titles -- <path to titles.tsv>
//! where each line is `app<TAB>title<TAB>count`.

use std::collections::BTreeMap;

use localtrack_core::context::{parse_context, ContextKind};

fn main() {
    let path = std::env::args()
        .nth(1)
        .expect("give me a tsv of app<TAB>title<TAB>count");
    let text = std::fs::read_to_string(path).expect("readable file");

    let mut by_workspace: BTreeMap<(ContextKind, String), i64> = BTreeMap::new();
    let mut unread: Vec<(String, String, i64)> = Vec::new();

    for line in text.lines() {
        let mut parts = line.split('\t');
        let (app, title, count) = match (parts.next(), parts.next(), parts.next()) {
            (Some(a), Some(t), Some(c)) => (a, t, c.parse::<i64>().unwrap_or(0)),
            _ => continue,
        };
        let domain = parts.next().filter(|d| !d.is_empty());
        let context = parse_context(Some(app), None, Some(title), domain, None);
        match context.workspace {
            Some(workspace) => *by_workspace.entry((context.kind, workspace)).or_default() += count,
            None => unread.push((app.to_string(), title.to_string(), count)),
        }
    }

    let mut rows: Vec<_> = by_workspace.into_iter().collect();
    rows.sort_by_key(|((_, _), count)| -*count);
    println!("== workspaces read from titles ==");
    for ((kind, workspace), count) in rows.iter().take(30) {
        println!("{count:5}  {:<9} {workspace}", kind.as_str());
    }

    unread.sort_by_key(|(_, _, count)| -*count);
    println!("\n== titles that said nothing ==");
    for (app, title, count) in unread.iter().take(15) {
        println!("{count:5}  {app:<20} {title}");
    }
}
