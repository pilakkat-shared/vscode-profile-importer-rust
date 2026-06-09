#!/usr/bin/env bash
set -euo pipefail

PROFILE_NAME="Rust Development"

echo "Checking VS Code installation..."

if ! command -v code >/dev/null 2>&1; then
  echo "Error: 'code' command not found."
  echo "Install VS Code and ensure the 'code' CLI is available."
  exit 1
fi

echo "Creating profile: $PROFILE_NAME"

# Create profile if it doesn't already exist
code --profile "$PROFILE_NAME" >/dev/null 2>&1 || true

echo "Installing extensions..."

EXTENSIONS=(
  "rust-lang.rust-analyzer"
  "vadimcn.vscode-lldb"
  "tamasfe.even-better-toml"
  "usernamehw.errorlens"
  "serayuzgur.crates"
  "eamodio.gitlens"
  "streetsidesoftware.code-spell-checker"
)

for ext in "${EXTENSIONS[@]}"; do
  echo "Installing $ext"
  code --profile "$PROFILE_NAME" --install-extension "$ext" --force
done

echo "Locating VS Code profile settings..."

PROFILE_DIR="${HOME}/.config/Code/User"

if [[ ! -d "$PROFILE_DIR" ]]; then
  echo "Could not find VS Code user directory:"
  echo "  $PROFILE_DIR"
  echo "Open VS Code once and create the profile manually if needed."
  exit 1
fi

PROFILE_ID=$(find "$PROFILE_DIR/profiles" -maxdepth 1 -mindepth 1 -type d | head -n 1)

if [[ -z "${PROFILE_ID:-}" ]]; then
  echo "Could not determine profile dir#!/usr/bin/env bash
set -euo pipefail

PROFILE_NAME="Rust Development"

echo "Checking VS Code installation..."

if ! command -v code >/dev/null 2>&1; then
    echo "Error: 'code' command not found."
    echo "Install VS Code and ensure the 'code' CLI is available."
    exit 1
fi

echo "Creating profile: $PROFILE_NAME"

# Create profile if it doesn't already exist
code --profile "$PROFILE_NAME" >/dev/null 2>&1 || true

echo "Installing extensions..."

EXTENSIONS=(
    "rust-lang.rust-analyzer"
    "vadimcn.vscode-lldb"
    "tamasfe.even-better-toml"
    "usernamehw.errorlens"
    "serayuzgur.crates"
    "eamodio.gitlens"
    "streetsidesoftware.code-spell-checker"
)

for ext in "${EXTENSIONS[@]}"; do
    echo "Installing $ext"
    code --profile "$PROFILE_NAME" --install-extension "$ext" --force
done

echo "Locating VS Code profile settings..."

PROFILE_DIR="${HOME}/.config/Code/User"

if [[ ! -d "$PROFILE_DIR" ]]; then
    echo "Could not find VS Code user directory:"
    echo " $PROFILE_DIR"
    echo "Open VS Code once and create the profile manually if needed."
    exit 1
fi

PROFILE_ID=$(find "$PROFILE_DIR/profiles" -maxdepth 1 -mindepth 1 -type d | head -n 1)

if [[ -z "${PROFILE_ID:-}" ]]; then
    echo "Could not determine profile directory."
    echo "Launch VS Code and switch to the profile once, then rerun."
    exit 1
fi

mkdir -p "$PROFILE_ID"

cat > "$PROFILE_ID/settings.json" <<'JSON'
{
    "editor.formatOnSave": true,
    "editor.rulers": [100],
    "files.trimTrailingWhitespace": true,

    "[rust]": {
        "editor.defaultFormatter": "rust-lang.rust-analyzer"
    },

    "rust-analyzer.check.command": "clippy",
    "rust-analyzer.cargo.features": "all",
    "rust-analyzer.procMacro.enable": true,

    "editor.inlayHints.enabled": "on",
    "rust-analyzer.inlayHints.bindingModeHints.enable": true,
    "rust-analyzer.inlayHints.closureReturnTypeHints.enable": "always",

    "terminal.integrated.defaultProfile.linux": "bash"
}
JSON

echo "Profile settings written to:"
echo " $PROFILE_ID/settings.json"

echo
echo "Rust Development profile configured successfully."
echo
echo "Open VS Code with:"
echo " code --profile \"$PROFILE_NAME\""ectory."
  echo "Launch VS Code and switch to the profile once, then rerun."
  exit 1
fi

mkdir -p "$PROFILE_ID"

cat >"$PROFILE_ID/settings.json" <<'JSON'
{
    "editor.formatOnSave": true,
    "editor.rulers": [100],
    "files.trimTrailingWhitespace": true,

    "[rust]": {
        "editor.defaultFormatter": "rust-lang.rust-analyzer"
    },

    "rust-analyzer.check.command": "clippy",
    "rust-analyzer.cargo.features": "all",
    "rust-analyzer.procMacro.enable": true,

    "editor.inlayHints.enabled": "on",
    "rust-analyzer.inlayHints.bindingModeHints.enable": true,
    "rust-analyzer.inlayHints.closureReturnTypeHints.enable": "always",

    "terminal.integrated.defaultProfile.linux": "bash"
}
JSON

echo "Profile settings written to:"
echo "  $PROFILE_ID/settings.json"

echo
echo "Rust Development profile configured successfully."
echo
echo "Open VS Code with:"
echo "  code --profile \"$PROFILE_NAME\""
