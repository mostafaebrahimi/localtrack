-- Migration 007_status_indexes: indexes for the once-a-second status read.
--
-- The tray, the floating timer and the status card all ask what is happening
-- right now, once a second, for as long as LocalTrack runs. Both of those
-- lookups search by when a segment *ended*, and nothing indexed that: the
-- newest-activity query fell back to a full scan of activity_segments plus a
-- temporary b-tree for the ORDER BY, so the cost of idling grew with every
-- segment ever recorded. Measured on the real schema, the pair cost 0.2 ms at
-- 2.5k rows and 30 ms at 300k -- three percent of a core, spent holding the
-- database lock that collection needs.

-- "What is the user doing right now": newest non-idle segment.
CREATE INDEX idx_segments_ended ON activity_segments(ended_at_ms);

-- "When did Chrome last report anything": leading source column narrows to the
-- browser first, then the range on ended_at_ms. started_at_ms is carried so the
-- query's guard against future-dated segments is answered from the index too,
-- leaving the whole lookup covering.
CREATE INDEX idx_segments_source_ended
    ON activity_segments(source, ended_at_ms, started_at_ms);
