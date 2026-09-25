//! Maximum metadata lengths enforced before anything is persisted (spec §106).

pub const APP_NAME_MAX: usize = 255;
pub const PROCESS_NAME_MAX: usize = 255;
pub const WINDOW_TITLE_MAX: usize = 2048;
pub const DOMAIN_MAX: usize = 255;
pub const URL_MAX: usize = 8192;
pub const PAGE_TITLE_MAX: usize = 2048;
pub const INTERACTION_LABEL_MAX: usize = 120;

/// Maximum size of a single native message payload (defensive bound).
pub const NATIVE_MESSAGE_MAX_BYTES: usize = 1024 * 1024;

/// Truncate on a character boundary, never in the middle of a UTF-8 sequence.
pub fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    value.chars().take(max).collect()
}

/// Collapse runs of whitespace and trim; used for titles and labels.
pub fn normalize_whitespace(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncates_on_char_boundary() {
        assert_eq!(truncate("héllo", 3), "hél");
        assert_eq!(truncate("hello", 50), "hello");
    }

    #[test]
    fn normalizes_whitespace() {
        assert_eq!(normalize_whitespace("  Save   \n Invoice "), "Save Invoice");
    }
}
