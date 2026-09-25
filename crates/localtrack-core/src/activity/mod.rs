//! Activity domain: sources, kinds, normalized observations, segments and the
//! heartbeat/merge engine (spec §9–§12, §29, §69).

pub mod merge;
pub mod observation;
pub mod segment;

pub use merge::{MergeConfig, SegmentMerger, StreamId};
pub use observation::{
    BrowserInteractionObservation, BrowserObservation, IdleObservation, InteractionType,
    Observation, SystemLockObservation, WindowObservation,
};
pub use segment::{
    ActivityKind, ActivitySegment, ActivitySource, ClassificationSource, SegmentKey,
};
