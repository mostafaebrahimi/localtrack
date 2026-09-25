#!/usr/bin/env bash
# Removes the LocalTrack native messaging host registration.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="$root/target/release/localtrack-native-host"

if [ -x "$binary" ]; then
  "$binary" uninstall
else
  # Fall back to removing the manifests directly.
  config="${XDG_CONFIG_HOME:-$HOME/.config}"
  for dir in "$config/google-chrome/NativeMessagingHosts" "$config/chromium/NativeMessagingHosts"; do
    manifest="$dir/com.localtrack.native.json"
    if [ -f "$manifest" ]; then
      rm -f "$manifest"
      echo "removed $manifest"
    fi
  done
fi
