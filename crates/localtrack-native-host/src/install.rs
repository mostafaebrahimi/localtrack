//! Native messaging host registration (spec §103–§105).
//!
//! Current-user installation is used wherever possible, and `allowed_origins`
//! always names the exact extension id — never a wildcard.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const HOST_NAME: &str = "com.localtrack.native";

/// Firefox identifies callers by add-on id rather than by origin, so its
/// manifest uses `allowed_extensions` where Chrome uses `allowed_origins`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostManifest {
    pub name: String,
    pub description: String,
    pub path: String,
    #[serde(rename = "type")]
    pub host_type: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_origins: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed_extensions: Vec<String>,
}

/// The default add-on id built into the Firefox manifest.
pub const FIREFOX_EXTENSION_ID: &str = "localtrack@localtrack.app";

impl HostManifest {
    pub fn new(executable: &Path, extension_ids: &[String]) -> Self {
        Self {
            name: HOST_NAME.to_string(),
            description: "LocalTrack Native Messaging Host".to_string(),
            path: executable.display().to_string(),
            host_type: "stdio".to_string(),
            allowed_origins: extension_ids
                .iter()
                .map(|id| format!("chrome-extension://{id}/"))
                .collect(),
            allowed_extensions: Vec::new(),
        }
    }

    /// The Firefox flavour of the same registration.
    pub fn for_firefox(executable: &Path, extension_ids: &[String]) -> Self {
        let ids: Vec<String> = if extension_ids.is_empty() {
            vec![FIREFOX_EXTENSION_ID.to_string()]
        } else {
            extension_ids.to_vec()
        };
        Self {
            name: HOST_NAME.to_string(),
            description: "LocalTrack Native Messaging Host".to_string(),
            path: executable.display().to_string(),
            host_type: "stdio".to_string(),
            allowed_origins: Vec::new(),
            allowed_extensions: ids,
        }
    }
}

/// Firefox's per-user native messaging host directories.
pub fn firefox_manifest_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    #[cfg(target_os = "linux")]
    {
        if let Ok(home) = std::env::var("HOME") {
            dirs.push(PathBuf::from(&home).join(".mozilla/native-messaging-hosts"));
        }
    }

    #[cfg(windows)]
    {
        if let Ok(appdata) = std::env::var("APPDATA") {
            dirs.push(PathBuf::from(appdata).join("LocalTrack"));
        }
    }

    dirs
}

/// Chrome's per-user native messaging host directories.
pub fn manifest_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();

    #[cfg(target_os = "linux")]
    {
        let config = std::env::var("XDG_CONFIG_HOME")
            .ok()
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| {
                std::env::var("HOME")
                    .ok()
                    .map(|h| PathBuf::from(h).join(".config"))
            });
        if let Some(config) = config {
            dirs.push(config.join("google-chrome/NativeMessagingHosts"));
            dirs.push(config.join("chromium/NativeMessagingHosts"));
        }
    }

    #[cfg(windows)]
    {
        // On Windows the manifest lives next to the executable and is
        // referenced from the registry.
        if let Ok(appdata) = std::env::var("APPDATA") {
            dirs.push(PathBuf::from(appdata).join("LocalTrack"));
        }
    }

    dirs
}

/// Write the manifest for every Chrome-family directory that exists.
pub fn install(executable: &Path, extension_ids: &[String]) -> std::io::Result<Vec<PathBuf>> {
    let mut written = install_chrome(executable, extension_ids)?;
    written.extend(install_firefox(executable, &[])?);
    Ok(written)
}

/// Register for Firefox. With no ids the built-in add-on id is used.
pub fn install_firefox(
    executable: &Path,
    extension_ids: &[String],
) -> std::io::Result<Vec<PathBuf>> {
    let manifest = HostManifest::for_firefox(executable, extension_ids);
    let body = serde_json::to_string_pretty(&manifest)?;
    let mut written = Vec::new();

    for dir in firefox_manifest_dirs() {
        let parent_exists = dir.parent().map(|p| p.exists()).unwrap_or(false);
        if !parent_exists && !cfg!(windows) {
            continue;
        }
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{HOST_NAME}.json"));
        std::fs::write(&path, &body)?;
        written.push(path);
    }

    #[cfg(windows)]
    {
        if let Some(path) = written.first() {
            register_windows_firefox(path)?;
        }
    }

    Ok(written)
}

fn install_chrome(executable: &Path, extension_ids: &[String]) -> std::io::Result<Vec<PathBuf>> {
    let manifest = HostManifest::new(executable, extension_ids);
    let body = serde_json::to_string_pretty(&manifest)?;
    let mut written = Vec::new();

    for dir in manifest_dirs() {
        // Only install for browsers the user actually has, except on Windows
        // where the single application directory is always created.
        let parent_exists = dir.parent().map(|p| p.exists()).unwrap_or(false);
        if !parent_exists && !cfg!(windows) {
            continue;
        }
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{HOST_NAME}.json"));
        std::fs::write(&path, &body)?;
        written.push(path);
    }

    #[cfg(windows)]
    {
        if let Some(path) = written.first() {
            register_windows(path)?;
        }
    }

    Ok(written)
}

pub fn uninstall() -> std::io::Result<Vec<PathBuf>> {
    let mut removed = Vec::new();
    for dir in manifest_dirs().into_iter().chain(firefox_manifest_dirs()) {
        let path = dir.join(format!("{HOST_NAME}.json"));
        if path.exists() {
            std::fs::remove_file(&path)?;
            removed.push(path);
        }
    }
    Ok(removed)
}

#[cfg(windows)]
fn register_windows_firefox(manifest_path: &Path) -> std::io::Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(format!(
        "Software\\Mozilla\\NativeMessagingHosts\\{HOST_NAME}"
    ))?;
    key.set_value("", &manifest_path.display().to_string())?;
    Ok(())
}

#[cfg(windows)]
fn register_windows(manifest_path: &Path) -> std::io::Result<()> {
    use winreg::enums::HKEY_CURRENT_USER;
    use winreg::RegKey;

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(format!(
        "Software\\Google\\Chrome\\NativeMessagingHosts\\{HOST_NAME}"
    ))?;
    key.set_value("", &manifest_path.display().to_string())?;
    Ok(())
}

/// Check whether a caller is allowed by an installed manifest.
///
/// Chrome passes an origin (`chrome-extension://ID/`), Firefox passes the
/// add-on id itself, so both forms are checked against the right list.
pub fn origin_allowed(manifest: &HostManifest, origin: &str) -> bool {
    let normalized = origin.trim_end_matches('/');
    manifest
        .allowed_origins
        .iter()
        .any(|allowed| allowed.trim_end_matches('/') == normalized)
        || manifest
            .allowed_extensions
            .iter()
            .any(|allowed| allowed == normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_names_the_exact_extension() {
        let manifest = HostManifest::new(
            Path::new("/usr/local/bin/localtrack-native-host"),
            &["abcdefghijklmnopabcdefghijklmnop".to_string()],
        );
        assert_eq!(manifest.name, HOST_NAME);
        assert_eq!(manifest.host_type, "stdio");
        assert_eq!(
            manifest.allowed_origins,
            vec!["chrome-extension://abcdefghijklmnopabcdefghijklmnop/"]
        );
        assert!(!manifest.allowed_origins.iter().any(|o| o.contains('*')));
    }

    #[test]
    fn firefox_registration_uses_extension_ids() {
        let manifest = HostManifest::for_firefox(Path::new("/usr/local/bin/host"), &[]);
        assert!(manifest.allowed_origins.is_empty());
        assert_eq!(manifest.allowed_extensions, vec![FIREFOX_EXTENSION_ID]);
        assert!(origin_allowed(&manifest, FIREFOX_EXTENSION_ID));
        assert!(!origin_allowed(&manifest, "someone@else"));

        let json = serde_json::to_string(&manifest).unwrap();
        assert!(json.contains("allowed_extensions"));
        assert!(
            !json.contains("allowed_origins"),
            "Firefox rejects the Chrome key"
        );
    }

    #[test]
    fn origin_matching_ignores_trailing_slash_only() {
        let manifest = HostManifest::new(Path::new("/bin/host"), &["abc".to_string()]);
        assert!(origin_allowed(&manifest, "chrome-extension://abc/"));
        assert!(origin_allowed(&manifest, "chrome-extension://abc"));
        assert!(!origin_allowed(&manifest, "chrome-extension://other/"));
        assert!(!origin_allowed(&manifest, "https://evil.example.com"));
    }
}
