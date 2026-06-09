#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CARGO_MANIFEST="$ROOT_DIR/Cargo.toml"

usage() {
  cat <<EOF
Usage: $(basename "$0") [--packaging] [--real-vscode] [--all]

Run the test suite with helpers for guarded integration tests.

Options:
  --packaging     Run the packaging integration test (requires ar and tar).
  --real-vscode   Run the real VS Code detection test (requires 'code' CLI).
  --all           Run normal tests, then packaging and real-vscode tests if available.
  (no args)       Run the standard cargo test (guarded tests are skipped by default).

Examples:
  # run unit + default integration tests (guarded tests skipped)
  $(basename "$0")

  # run only the packaging test
  $(basename "$0") --packaging

  # run both guarded tests (if prerequisites installed)
  $(basename "$0") --all

EOF
}

run_default() {
  echo "Running default test suite..."
  cargo test --manifest-path "$CARGO_MANIFEST" -- --nocapture
}

run_packaging() {
  echo "Running packaging integration test..."
  for cmd in ar tar bash; do
    if ! command -v "$cmd" >/dev/null 2>&1; then
      echo "Required command not found: $cmd" >&2
      echo "Please install binutils (for ar) and tar. On Debian/Ubuntu: apt install binutils tar" >&2
      return 1
    fi
  done

  PACKAGING_TEST=1 cargo test --manifest-path "$CARGO_MANIFEST" --test packaging_integration -- --nocapture
}

run_real_vscode() {
  echo "Running real VS Code detection test..."
  if ! command -v code >/dev/null 2>&1; then
    echo "VS Code 'code' CLI not found in PATH." >&2
    echo "Install VS Code or ensure the 'code' command is available." >&2
    return 1
  fi

  REAL_VSCODE_TEST=1 cargo test --manifest-path "$CARGO_MANIFEST" --test real_vscode_detection -- --nocapture
}

if [ "$#" -eq 0 ]; then
  run_default
  exit $?
fi

while [ "$#" -gt 0 ]; do
  case "$1" in
    --packaging)
      run_packaging
      shift
      ;;
    --real-vscode)
      run_real_vscode
      shift
      ;;
    --all)
      run_default
      run_packaging || true
      run_real_vscode || true
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown arg: $1" >&2
      usage
      exit 2
      ;;
  esac
done
