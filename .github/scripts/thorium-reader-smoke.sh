#!/usr/bin/env bash
set -euo pipefail

fixture_dir="${1:-target/epubcheck-fixtures}"
expected_version="${THORIUM_EXPECTED_VERSION:-3.5.1}"

if ! command -v thorium >/dev/null 2>&1; then
  echo "Thorium executable is not available on PATH." >&2
  exit 1
fi
if ! command -v xvfb-run >/dev/null 2>&1; then
  echo "xvfb-run is required for the headless Thorium smoke." >&2
  exit 1
fi

version_output="$(thorium --version 2>&1 | head -n 1)"
if [[ "$version_output" != *"$expected_version"* ]]; then
  echo "Expected Thorium $expected_version, got: $version_output" >&2
  exit 1
fi

mapfile -t fixtures < <(find "$fixture_dir" -maxdepth 1 -type f -name '*.epub' -print | sort)
if [[ "${#fixtures[@]}" -ne 3 ]]; then
  echo "Expected exactly three StoryTeller interoperability fixtures, found ${#fixtures[@]}." >&2
  exit 1
fi

for fixture in "${fixtures[@]}"; do
  fixture_name="$(basename "$fixture")"
  profile_root="$(mktemp -d)"
  home_dir="$profile_root/home"
  mkdir -p "$home_dir/.config" "$home_dir/.cache"

  stdout_log="$profile_root/thorium.stdout.log"
  stderr_log="$profile_root/thorium.stderr.log"

  echo "== Thorium 3.5.1 import/open smoke: $fixture_name =="

  HOME="$home_dir" \
  XDG_CONFIG_HOME="$home_dir/.config" \
  XDG_CACHE_HOME="$home_dir/.cache" \
  LIBGL_ALWAYS_SOFTWARE=1 \
    xvfb-run -a thorium "$fixture" >"$stdout_log" 2>"$stderr_log" &
  launcher_pid=$!

  sleep 12

  if ! kill -0 "$launcher_pid" 2>/dev/null; then
    echo "Thorium exited before the import/open observation window for $fixture_name." >&2
    echo "--- stdout ---" >&2
    cat "$stdout_log" >&2 || true
    echo "--- stderr ---" >&2
    cat "$stderr_log" >&2 || true
    wait "$launcher_pid" || true
    rm -rf "$profile_root"
    exit 1
  fi

  publication_dir="$home_dir/.config/EDRLab.ThoriumReader/publications"
  imported_file="$(find "$publication_dir" -type f -print -quit 2>/dev/null || true)"
  if [[ -z "$imported_file" ]]; then
    echo "Thorium stayed alive but did not persist an imported publication for $fixture_name." >&2
    echo "--- stdout ---" >&2
    cat "$stdout_log" >&2 || true
    echo "--- stderr ---" >&2
    cat "$stderr_log" >&2 || true
    kill "$launcher_pid" 2>/dev/null || true
    wait "$launcher_pid" || true
    rm -rf "$profile_root"
    exit 1
  fi

  kill "$launcher_pid" 2>/dev/null || true
  wait "$launcher_pid" || true
  rm -rf "$profile_root"
  echo "Thorium imported and kept the fixture open: $fixture_name"
done

echo "Thorium reading-system smoke passed for all three StoryTeller fixtures. This gate proves real-reader import/open behavior only; it does not claim automated audible Media Overlay playback verification."
