//! Layer 1 support — the collector interface — plus the event processing
//! pipeline that turns observations into stored segments (spec §7, §8, §68).
//!
//! Collectors know nothing about reporting or storage. They publish normalized
//! [`Observation`](localtrack_core::activity::Observation) values; the pipeline
//! validates, normalizes, applies tracking state and privacy rules, merges,
//! classifies and finally persists them.

pub mod afk;
pub mod collector;
pub mod pipeline;
pub mod supervisor;

pub use afk::AfkTracker;
pub use collector::{ActivityCollector, CollectorHandle, CollectorStatus, ObservationSender};
pub use pipeline::{IngestPipeline, PipelineStats, TrackingDecision};
pub use supervisor::Backoff;
