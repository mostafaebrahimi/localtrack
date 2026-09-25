//! Privacy layer (spec §43–§51, §67, §68).
//!
//! Privacy filtering happens **before** anything durable is written. Nothing in
//! this module ever returns a value that still contains a query string,
//! credential or excluded target when policy says it must not.

pub mod exclusion;
pub mod url;

pub use exclusion::{
    ExclusionAction, ExclusionDecision, ExclusionMatcher, ExclusionRule, ExclusionTarget,
};
pub use url::{domain_of, sanitize_url, UrlPolicy};
