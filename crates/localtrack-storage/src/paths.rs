//! Local file locations. Everything LocalTrack writes lives under one
//! per-user directory; nothing is ever written outside the machine.

use std::path::PathBuf;

/// `%APPDATA%\LocalTrack` on Windows, `$XDG_DATA_HOME/localtrack` (or
/// `~/.local/share/localtrack`) on Linux.
pub fn data_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("LOCALTRACK_DATA_DIR") {
        if !custom.is_empty() {
            return PathBuf::from(custom);
        }
    }

    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            return PathBuf::from(appdata).join("LocalTrack");
        }
    }

    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("localtrack");
        }
    }
    if let Ok(home) = std::env::var("HOME") {
        return PathBuf::from(home).join(".local/share/localtrack");
    }
    PathBuf::from(".localtrack")
}

pub fn db_path() -> PathBuf {
    data_dir().join(crate::DB_FILE_NAME)
}

pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

pub fn backups_dir() -> PathBuf {
    data_dir().join("backups")
}

pub fn ensure_dirs() -> std::io::Result<()> {
    std::fs::create_dir_all(data_dir())?;
    std::fs::create_dir_all(logs_dir())?;
    std::fs::create_dir_all(backups_dir())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn honours_an_explicit_data_dir() {
        std::env::set_var("LOCALTRACK_DATA_DIR", "/tmp/localtrack-test");
        assert_eq!(data_dir(), PathBuf::from("/tmp/localtrack-test"));
        assert!(db_path().ends_with("localtrack.db"));
        std::env::remove_var("LOCALTRACK_DATA_DIR");
    }
}
