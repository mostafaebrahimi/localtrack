//! Two-way synchronisation of work sessions.
//!
//! Someone may start a timer in the web application and stop it on their
//! laptop, or work offline all day and reconcile later. Both sides therefore
//! hold authority over the same records, and this module decides — as pure
//! functions, so the rules can be tested exhaustively — what the result is.
//!
//! Local-only use is unaffected: nothing here runs unless the device is
//! enrolled.

use serde::{Deserialize, Serialize};

use crate::sessions::{WorkBreak, WorkSession};

/// One work session as it travels between the device and the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSyncItem {
    /// The server's identifier, absent for a session created offline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_id: Option<String>,
    /// The device's identifier, echoed back by the server so a session created
    /// offline is matched to its server row exactly once.
    pub client_id: String,
    pub started_at_ms: i64,
    #[serde(default)]
    pub ended_at_ms: Option<i64>,
    /// What the person was working on.
    #[serde(default)]
    pub note: Option<String>,
    #[serde(default)]
    pub breaks: Vec<BreakSyncItem>,
    pub updated_at_ms: i64,
    /// Set when the record was deleted; a tombstone travels like any change.
    #[serde(default)]
    pub deleted_at_ms: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakSyncItem {
    pub started_at_ms: i64,
    #[serde(default)]
    pub ended_at_ms: Option<i64>,
}

impl SessionSyncItem {
    pub fn from_local(
        session: &WorkSession,
        breaks: &[WorkBreak],
        remote_id: Option<String>,
    ) -> Self {
        Self {
            remote_id,
            client_id: session.id.clone(),
            started_at_ms: session.started_at_ms,
            ended_at_ms: session.ended_at_ms,
            note: session.note.clone(),
            breaks: breaks
                .iter()
                .map(|b| BreakSyncItem {
                    started_at_ms: b.started_at_ms,
                    ended_at_ms: b.ended_at_ms,
                })
                .collect(),
            updated_at_ms: session.updated_at_ms,
            deleted_at_ms: None,
        }
    }

    pub fn is_tombstone(&self) -> bool {
        self.deleted_at_ms.is_some()
    }

    /// A session still running has no end; only one may be open at a time.
    pub fn is_open(&self) -> bool {
        self.ended_at_ms.is_none() && !self.is_tombstone()
    }
}

/// What the device should do with one incoming record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum MergeAction {
    /// The record is new here.
    Insert,
    /// The remote version is newer and replaces the local one.
    Update,
    /// The record was deleted remotely.
    Delete,
    /// The local version is newer; keep it and push it again.
    KeepLocal,
    /// Nothing to do.
    Skip,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeOutcome {
    pub action: MergeAction,
    pub item: SessionSyncItem,
}

/// The local side of a record, as far as merging is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LocalRecord {
    pub client_id: String,
    pub remote_id: Option<String>,
    pub updated_at_ms: i64,
    /// True when the device has unsent changes for this record.
    pub dirty: bool,
    pub deleted: bool,
}

/// Decide what to do with one incoming record.
///
/// The rule is last-writer-wins on `updated_at_ms`, with two refinements:
/// a local edit that has not reached the server yet wins ties, so a change made
/// on the device is never silently discarded; and a tombstone always wins over
/// an equal-or-older edit, because resurrecting a deleted record is worse than
/// losing a late edit to it.
pub fn merge_incoming(remote: &SessionSyncItem, local: Option<&LocalRecord>) -> MergeOutcome {
    let Some(local) = local else {
        // Never resurrect something deleted before it was ever stored here.
        let action = if remote.is_tombstone() {
            MergeAction::Skip
        } else {
            MergeAction::Insert
        };
        return MergeOutcome {
            action,
            item: remote.clone(),
        };
    };

    if local.deleted && !remote.is_tombstone() && remote.updated_at_ms <= local.updated_at_ms {
        return MergeOutcome {
            action: MergeAction::Skip,
            item: remote.clone(),
        };
    }

    if remote.is_tombstone() {
        let action = if local.deleted {
            MergeAction::Skip
        } else if local.dirty && local.updated_at_ms > remote.deleted_at_ms.unwrap_or(0) {
            MergeAction::KeepLocal
        } else {
            MergeAction::Delete
        };
        return MergeOutcome {
            action,
            item: remote.clone(),
        };
    }

    let action = if remote.updated_at_ms > local.updated_at_ms {
        MergeAction::Update
    } else if local.dirty {
        MergeAction::KeepLocal
    } else {
        MergeAction::Skip
    };

    MergeOutcome {
        action,
        item: remote.clone(),
    }
}

/// Apply a batch, preserving input order.
pub fn merge_batch(
    remote: &[SessionSyncItem],
    lookup: impl Fn(&SessionSyncItem) -> Option<LocalRecord>,
) -> Vec<MergeOutcome> {
    remote
        .iter()
        .map(|item| {
            let local = lookup(item);
            merge_incoming(item, local.as_ref())
        })
        .collect()
}

/// Reject a batch that would leave two sessions running at once.
///
/// Only one session may be open (spec §15), and a server that believes
/// otherwise must not be able to corrupt the device's clock state.
pub fn resolve_open_sessions(items: &mut [SessionSyncItem]) -> usize {
    let mut open: Vec<usize> = items
        .iter()
        .enumerate()
        .filter(|(_, item)| item.is_open())
        .map(|(index, _)| index)
        .collect();

    if open.len() <= 1 {
        return 0;
    }

    // Keep the one that started most recently; close the rest at the next
    // session's start so no time is invented.
    open.sort_by_key(|index| items[*index].started_at_ms);
    let keep = *open.last().expect("open sessions are non-empty");
    let mut closed = 0;

    for window in open.windows(2) {
        let (current, next) = (window[0], window[1]);
        if current == keep {
            continue;
        }
        items[current].ended_at_ms = Some(items[next].started_at_ms);
        closed += 1;
    }
    closed
}

#[cfg(test)]
mod tests {
    use super::*;

    fn remote(client_id: &str, updated: i64) -> SessionSyncItem {
        SessionSyncItem {
            remote_id: Some(format!("r-{client_id}")),
            client_id: client_id.to_string(),
            started_at_ms: 1_000,
            ended_at_ms: Some(2_000),
            note: Some("Invoice run".into()),
            breaks: vec![],
            updated_at_ms: updated,
            deleted_at_ms: None,
        }
    }

    fn local(updated: i64, dirty: bool) -> LocalRecord {
        LocalRecord {
            client_id: "s1".into(),
            remote_id: Some("r-s1".into()),
            updated_at_ms: updated,
            dirty,
            deleted: false,
        }
    }

    #[test]
    fn an_unknown_record_is_inserted() {
        let outcome = merge_incoming(&remote("s1", 10), None);
        assert_eq!(outcome.action, MergeAction::Insert);
    }

    #[test]
    fn a_newer_remote_edit_wins() {
        let outcome = merge_incoming(&remote("s1", 20), Some(&local(10, false)));
        assert_eq!(outcome.action, MergeAction::Update);
    }

    #[test]
    fn an_unsent_local_edit_is_never_discarded() {
        // Same timestamp, local has unsent changes: the device keeps its version.
        let outcome = merge_incoming(&remote("s1", 10), Some(&local(10, true)));
        assert_eq!(outcome.action, MergeAction::KeepLocal);

        // And a stale remote never overwrites a newer local edit.
        let outcome = merge_incoming(&remote("s1", 5), Some(&local(10, true)));
        assert_eq!(outcome.action, MergeAction::KeepLocal);
    }

    #[test]
    fn an_identical_clean_record_does_nothing() {
        let outcome = merge_incoming(&remote("s1", 10), Some(&local(10, false)));
        assert_eq!(outcome.action, MergeAction::Skip);
    }

    #[test]
    fn tombstones_delete_unless_the_device_edited_later() {
        let mut tombstone = remote("s1", 30);
        tombstone.deleted_at_ms = Some(30);

        assert_eq!(
            merge_incoming(&tombstone, Some(&local(10, false))).action,
            MergeAction::Delete
        );
        assert_eq!(
            merge_incoming(&tombstone, Some(&local(40, true))).action,
            MergeAction::KeepLocal,
            "an edit made after the deletion is not thrown away silently"
        );

        let already_deleted = LocalRecord {
            deleted: true,
            ..local(10, false)
        };
        assert_eq!(
            merge_incoming(&tombstone, Some(&already_deleted)).action,
            MergeAction::Skip
        );
    }

    #[test]
    fn a_deleted_record_is_not_resurrected() {
        let deleted_here = LocalRecord {
            deleted: true,
            updated_at_ms: 50,
            ..local(50, false)
        };
        assert_eq!(
            merge_incoming(&remote("s1", 20), Some(&deleted_here)).action,
            MergeAction::Skip
        );
        // Unless the server genuinely has a later version of it.
        assert_eq!(
            merge_incoming(&remote("s1", 90), Some(&deleted_here)).action,
            MergeAction::Update
        );
    }

    #[test]
    fn a_tombstone_for_something_never_seen_is_ignored() {
        let mut tombstone = remote("s9", 10);
        tombstone.deleted_at_ms = Some(10);
        assert_eq!(merge_incoming(&tombstone, None).action, MergeAction::Skip);
    }

    #[test]
    fn only_one_session_may_be_left_running() {
        let mut items = vec![
            SessionSyncItem {
                ended_at_ms: None,
                started_at_ms: 1_000,
                ..remote("a", 1)
            },
            SessionSyncItem {
                ended_at_ms: None,
                started_at_ms: 5_000,
                ..remote("b", 2)
            },
            SessionSyncItem {
                ended_at_ms: None,
                started_at_ms: 9_000,
                ..remote("c", 3)
            },
        ];
        let closed = resolve_open_sessions(&mut items);
        assert_eq!(closed, 2);
        assert_eq!(
            items[0].ended_at_ms,
            Some(5_000),
            "closed where the next one began"
        );
        assert_eq!(items[1].ended_at_ms, Some(9_000));
        assert_eq!(
            items[2].ended_at_ms, None,
            "the newest session keeps running"
        );
    }

    #[test]
    fn a_single_open_session_is_left_alone() {
        let mut items = vec![SessionSyncItem {
            ended_at_ms: None,
            ..remote("a", 1)
        }];
        assert_eq!(resolve_open_sessions(&mut items), 0);
        assert!(items[0].is_open());
    }

    #[test]
    fn batches_preserve_order() {
        let items = vec![remote("a", 1), remote("b", 2)];
        let outcomes = merge_batch(&items, |_| None);
        assert_eq!(outcomes.len(), 2);
        assert_eq!(outcomes[0].item.client_id, "a");
        assert!(outcomes.iter().all(|o| o.action == MergeAction::Insert));
    }
}
