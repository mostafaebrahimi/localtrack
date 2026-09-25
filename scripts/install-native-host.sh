#!/usr/bin/env bash
# Registers the LocalTrack native messaging host for Chrome (current user).
#
# Usage: scripts/install-native-host.sh <chrome-extension-id> [more ids...]
set -euo pipefail

if [ "$#" -lt 1 ]; then
  echo "usage: $0 <chrome-extension-id> [more ids...]" >&2
  echo "The id is shown on chrome://extensions with developer mode enabled." >&2
  exit 2
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
binary="$root/target/release/localtrack-native-host"

if [ ! -x "$binary" ]; then
  echo "Building the native host…"
  (cd "$root" && cargo build --release -p localtrack-native-host)
fi

"$binary" install "$@"
echo
echo "Reload the extension in chrome://extensions; the popup should show Connected."
