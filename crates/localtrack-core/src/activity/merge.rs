use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::segment::{ActivitySegment, SegmentKey};

/// Independent streams of activity. Each stream has at most one open segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamId {
    /// Desktop foreground window.
    Desktop,
    /// Chrome active page.
    Browser,
    /// Idle / locked state produced by the AFK and lock collectors.
    System,
}

impl StreamId {
    pub fn as_str(&self) -> &'static str {
        match self {
            StreamId::Desktop => "desktop",
            StreamId::Browser => "browser",
            StreamId::System => "system",
        }
    }
}

/// Why an open segment was closed. Boundaries never merge (spec §69).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CloseReason {
    MetadataChanged,
    Boundary,
    Stale,
    Shutdown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MergeConfig {
    /// Adjacent observations with the same metadata merge when the gap is at
    /// most this long (spec §69, default 20s).
    pub merge_gap_ms: i64,
    /// A stream with no heartbeat for this long is closed at its last heartbeat
    /// (spec §143/§144, default 30s).
    pub heartbeat_tolerance_ms: i64,
    /// Segments shorter than this are discarded as noise.
    pub min_segment_ms: i64,
}

impl Default for MergeConfig {
    fn default() -> Self {
        Self {
            merge_gap_ms: 20_000,
            heartbeat_tolerance_ms: 30_000,
            min_segment_ms: 1_000,
        }
    }
}

/// A segment currently open in memory (spec §29).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenSegment {
    pub id: String,
    pub key: SegmentKey,
    pub started_at_ms: i64,
    /// Last moment the activity was observed alive.
    pub last_seen_ms: i64,
    /// Last moment this segment was checkpointed to storage.
    pub last_checkpoint_ms: i64,
}

impl OpenSegment {
    pub fn duration_ms(&self) -> i64 {
        (self.last_seen_ms - self.started_at_ms).max(0)
    }
}

/// Turns a stream of observations into merged segments.
///
/// The merger holds one open segment per stream in memory and emits closed
/// segments for persistence. Nothing is fabricated: a segment can never extend
/// past the last moment its activity was actually observed.
#[derive(Debug, Default)]
pub struct SegmentMerger {
    config: MergeConfig,
    streams: HashMap<StreamId, OpenSegment>,
}

impl SegmentMerger {
    pub fn new(config: MergeConfig) -> Self {
        Self {
            config,
            streams: HashMap::new(),
        }
    }

    pub fn config(&self) -> &MergeConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: MergeConfig) {
        self.config = config;
    }

    pub fn open_segment(&self, stream: StreamId) -> Option<&OpenSegment> {
        self.streams.get(&stream)
    }

    pub fn open_segments(&self) -> impl Iterator<Item = (&StreamId, &OpenSegment)> {
        self.streams.iter()
    }

    /// Record an observation. Returns any segment that had to be closed.
    pub fn observe(
        &mut self,
        stream: StreamId,
        key: SegmentKey,
        at_ms: i64,
        now_ms: i64,
    ) -> Option<ActivitySegment> {
        let closed = match self.streams.get(&stream) {
            Some(open) if open.key == key => {
                let gap = at_ms - open.last_seen_ms;
                if gap <= self.config.merge_gap_ms && at_ms >= open.started_at_ms {
                    // Same metadata, small gap → extend the open segment.
                    if let Some(open) = self.streams.get_mut(&stream) {
                        open.last_seen_ms = open.last_seen_ms.max(at_ms);
                    }
                    return None;
                }
                // Same metadata but the gap is too large: do not bridge it.
                self.close(stream, open.last_seen_ms, now_ms, CloseReason::Stale)
            }
            Some(open) => {
                // Metadata changed. If the streams were contiguous, the previous
                // activity ended exactly when the new one started.
                let gap = at_ms - open.last_seen_ms;
                let end = if gap <= self.config.merge_gap_ms {
                    at_ms
                } else {
                    open.last_seen_ms
                };
                self.close(stream, end, now_ms, CloseReason::MetadataChanged)
            }
            None => None,
        };

        self.streams.insert(
            stream,
            OpenSegment {
                id: Uuid::new_v4().to_string(),
                key,
                started_at_ms: at_ms,
                last_seen_ms: at_ms,
                last_checkpoint_ms: at_ms,
            },
        );

        closed
    }

    /// Extend the open segment of a stream without changing metadata.
    pub fn heartbeat(&mut self, stream: StreamId, at_ms: i64) -> bool {
        match self.streams.get_mut(&stream) {
            Some(open) => {
                if at_ms > open.last_seen_ms {
                    open.last_seen_ms = at_ms;
                }
                true
            }
            None => false,
        }
    }

    /// Mark a stream's open segment as checkpointed to storage.
    pub fn mark_checkpointed(&mut self, stream: StreamId, at_ms: i64) {
        if let Some(open) = self.streams.get_mut(&stream) {
            open.last_checkpoint_ms = at_ms;
        }
    }

    /// Close a stream at `end_ms`, returning the segment when it is long enough.
    pub fn close(
        &mut self,
        stream: StreamId,
        end_ms: i64,
        now_ms: i64,
        _reason: CloseReason,
    ) -> Option<ActivitySegment> {
        let open = self.streams.remove(&stream)?;
        let end = end_ms.clamp(open.started_at_ms, i64::MAX);
        let mut segment = ActivitySegment::from_key(&open.key, open.started_at_ms, end, now_ms);
        segment.id = open.id;
        if segment.duration_ms() < self.config.min_segment_ms {
            return None;
        }
        Some(segment)
    }

    /// Close every open stream, e.g. on break start, clock out or shutdown
    /// (spec §19, §69). Boundaries must never be merged across.
    pub fn close_all(
        &mut self,
        end_ms: i64,
        now_ms: i64,
        reason: CloseReason,
    ) -> Vec<ActivitySegment> {
        let streams: Vec<StreamId> = self.streams.keys().copied().collect();
        let mut out = Vec::new();
        for stream in streams {
            if let Some(segment) = self.close(stream, end_ms, now_ms, reason) {
                out.push(segment);
            }
        }
        out.sort_by_key(|s| s.started_at_ms);
        out
    }

    /// Close streams whose heartbeat has stopped arriving (spec §143/§144).
    ///
    /// The segment ends at its last heartbeat, never at "now": a browser that
    /// disappeared is not still being used.
    pub fn flush_stale(&mut self, now_ms: i64, wall_now_ms: i64) -> Vec<ActivitySegment> {
        let stale: Vec<(StreamId, i64)> = self
            .streams
            .iter()
            .filter(|(_, open)| now_ms - open.last_seen_ms > self.config.heartbeat_tolerance_ms)
            .map(|(stream, open)| (*stream, open.last_seen_ms))
            .collect();
        let mut out = Vec::new();
        for (stream, last_seen) in stale {
            if let Some(segment) = self.close(stream, last_seen, wall_now_ms, CloseReason::Stale) {
                out.push(segment);
            }
        }
        out.sort_by_key(|s| s.started_at_ms);
        out
    }

    /// Streams that should be checkpointed now (spec §29 heartbeat persistence).
    pub fn due_for_checkpoint(&self, now_ms: i64, interval_ms: i64) -> Vec<StreamId> {
        self.streams
            .iter()
            .filter(|(_, open)| now_ms - open.last_checkpoint_ms >= interval_ms)
            .map(|(stream, _)| *stream)
            .collect()
    }

    /// A snapshot of an open segment as it would be persisted right now.
    pub fn snapshot(&self, stream: StreamId, now_ms: i64) -> Option<ActivitySegment> {
        let open = self.streams.get(&stream)?;
        let mut segment =
            ActivitySegment::from_key(&open.key, open.started_at_ms, open.last_seen_ms, now_ms);
        segment.id = open.id.clone();
        Some(segment)
    }

    pub fn is_empty(&self) -> bool {
        self.streams.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code_key(title: &str) -> SegmentKey {
        SegmentKey::desktop(
            Some("Visual Studio Code".into()),
            Some("Code.exe".into()),
            Some(title.into()),
        )
    }

    #[test]
    fn identical_observations_merge_into_one_segment() {
        let mut m = SegmentMerger::new(MergeConfig::default());
        assert!(m
            .observe(StreamId::Desktop, code_key("auth.ts"), 0, 0)
            .is_none());
        for t in (1_000..=60_000).step_by(1_000) {
            assert!(m
                .observe(StreamId::Desktop, code_key("auth.ts"), t, t)
                .is_none());
        }
        let closed = m.close_all(60_000, 60_000, CloseReason::Shutdown);
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].duration_ms(), 60_000);
        assert_eq!(closed[0].window_title.as_deref(), Some("auth.ts"));
    }

    #[test]
    fn metadata_change_closes_and_opens() {
        let mut m = SegmentMerger::new(MergeConfig::default());
        m.observe(StreamId::Desktop, code_key("a.ts"), 0, 0);
        let closed = m
            .observe(StreamId::Desktop, code_key("b.ts"), 10_000, 10_000)
            .expect("previous segment closed");
        assert_eq!(closed.started_at_ms, 0);
        assert_eq!(closed.ended_at_ms, 10_000);
        assert_eq!(closed.window_title.as_deref(), Some("a.ts"));
        assert_eq!(
            m.open_segment(StreamId::Desktop).unwrap().started_at_ms,
            10_000
        );
    }

    #[test]
    fn large_gap_is_not_bridged() {
        let mut m = SegmentMerger::new(MergeConfig::default());
        m.observe(StreamId::Desktop, code_key("a.ts"), 0, 0);
        m.heartbeat(StreamId::Desktop, 5_000);
        // 10 minutes later the same window is seen again: the gap stays untracked.
        let closed = m
            .observe(StreamId::Desktop, code_key("a.ts"), 605_000, 605_000)
            .expect("stale segment closed");
        assert_eq!(closed.ended_at_ms, 5_000);
        assert_eq!(
            m.open_segment(StreamId::Desktop).unwrap().started_at_ms,
            605_000
        );
    }

    #[test]
    fn stale_stream_is_closed_at_last_heartbeat_not_now() {
        let mut m = SegmentMerger::new(MergeConfig::default());
        m.observe(
            StreamId::Browser,
            SegmentKey::browser_page(
                Some("chrome".into()),
                Some("github.com".into()),
                Some("https://github.com/x".into()),
                Some("x".into()),
            ),
            0,
            0,
        );
        m.heartbeat(StreamId::Browser, 15_000);
        let flushed = m.flush_stale(300_000, 300_000);
        assert_eq!(flushed.len(), 1);
        assert_eq!(flushed[0].ended_at_ms, 15_000, "must not extend to now");
        assert!(m.is_empty());
    }

    #[test]
    fn boundaries_close_every_stream() {
        let mut m = SegmentMerger::new(MergeConfig::default());
        m.observe(StreamId::Desktop, code_key("a.ts"), 0, 0);
        m.observe(
            StreamId::Browser,
            SegmentKey::browser_page(Some("chrome".into()), Some("github.com".into()), None, None),
            0,
            0,
        );
        let closed = m.close_all(30_000, 30_000, CloseReason::Boundary);
        assert_eq!(closed.len(), 2);
        assert!(m.is_empty());
    }

    #[test]
    fn segments_below_minimum_are_dropped() {
        let mut m = SegmentMerger::new(MergeConfig::default());
        m.observe(StreamId::Desktop, code_key("a.ts"), 0, 0);
        let closed = m.close(StreamId::Desktop, 200, 200, CloseReason::Boundary);
        assert!(closed.is_none());
    }

    #[test]
    fn checkpoint_scheduling() {
        let mut m = SegmentMerger::new(MergeConfig::default());
        m.observe(StreamId::Desktop, code_key("a.ts"), 0, 0);
        assert!(m.due_for_checkpoint(10_000, 15_000).is_empty());
        assert_eq!(
            m.due_for_checkpoint(20_000, 15_000),
            vec![StreamId::Desktop]
        );
        m.mark_checkpointed(StreamId::Desktop, 20_000);
        assert!(m.due_for_checkpoint(25_000, 15_000).is_empty());
    }
}
