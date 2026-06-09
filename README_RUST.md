# vscode-profile-importer

A Rust CLI tool to import VS Code `.code-profile` files and manage profile
extensions from the command line.

Tested against VS Code 1.123.0 on Linux.

---

## Installation

### From source

```bash
cargo build --release
# binary at: target/release/vscode-profile-importer
```

### Debian package

```bash
./scripts/build_deb.sh
sudo dpkg -i target/debian/*.deb
```

---

## Requirements

- `code` (VS Code CLI) in `PATH`
- `~/.config/Code/User/globalStorage/storage.json` must exist  
  (open VS Code once to create it)
- On headless servers: a display must be available for profile creation,  
  or use `--no-use-code-cli` when the profile is already registered

---

## Subcommands

| Subcommand   | Description                                                   |
|---|---|
| `import`     | Import a `.code-profile` file into a new or existing profile  |
| `list`       | List all registered profiles with their extension counts      |
| `extensions` | List extensions installed in a specific profile               |
| `remove`     | Uninstall a single extension from a profile                   |
| `uninstall`  | Uninstall ALL extensions from a profile                       |

---

## import

Import a `.code-profile` file exported from VS Code.

**What it does:**

1. Parses the profile file (handles both extension-encoding variants and
   embedded control characters).
2. Creates the named profile via `setsid code --profile <name> <tmpdir> &`,
   polls `storage.json` until registered, then kills the window.
3. Installs each extension with
   `code --profile <name> --install-extension <id> --force`.
4. Resolves the profile's hashed directory from `storage.json` and writes
   `settings.json` / `keybindings.json` there.

```
vscode-profile-importer import <PROFILE_FILE> [OPTIONS]
```

| Option | Default | Description |
|---|---|---|
| `<PROFILE_FILE>` | *(required)* | Path to the `.code-profile` file |
| `--storage-json <PATH>` | `~/.config/Code/User/globalStorage/storage.json` | Override storage.json path |
| `--timeout <SECS>` | `120` | Per-extension install timeout |
| `--create-timeout <SECS>` | `30` | Profile creation timeout |
| `--report-path <PATH>` | *(none)* | Write a JSON import summary to this file |
| `--dry-run` | false | Print what would happen without doing it |
| `--non-interactive` | false | Overwrite existing profiles and skip failed extensions without prompting |
| `--no-use-code-cli` | false | Skip the profile creation step (use when profile already exists) |

**Examples:**

```bash
# Interactive import
vscode-profile-importer import ~/Downloads/MyProfile.code-profile

# CI / non-interactive
vscode-profile-importer import profile.code-profile --non-interactive

# Dry run to preview
vscode-profile-importer import profile.code-profile --dry-run

# Write a summary report
vscode-profile-importer import profile.code-profile --report-path /tmp/report.json
```

**Special cases:**

| Case | Behaviour |
|---|---|
| Profile `"Default"` | Installs extensions into the built-in default slot; no creation step |
| Profile already exists | Prompts to overwrite (skipped with `--non-interactive`) |
| Extension install fails | Prompts skip/retry/abort (auto-skips with `--non-interactive`) |
| No `settings` / `keybindings` in profile | Respective file write skipped |

---

## list

List all registered VS Code profiles with extension counts.

```
vscode-profile-importer list [--storage-json <PATH>]
```

**Example output:**

```
  PROFILE NAME      EXTS  ICON
  ──────────────────────────────
  Default              5    [default]
  Rust Dev Hub        32
  C/C++ Dev Hub       30
  Python Dev Hub      28  beaker
```

---

## extensions

List extensions installed in a specific profile.

```
vscode-profile-importer extensions --profile <NAME> [--storage-json <PATH>]
```

Uses `code [--profile <name>] --list-extensions` to retrieve the live list.

**Examples:**

```bash
vscode-profile-importer extensions --profile "Rust Dev Hub"
vscode-profile-importer extensions --profile Default
```

---

## remove

Uninstall a single extension from a profile, with confirmation.

```
vscode-profile-importer remove --profile <NAME> --extension <EXT-ID> [OPTIONS]
```

| Option | Description |
|---|---|
| `--profile <NAME>` | *(required)* Profile to remove from |
| `--extension <EXT-ID>` | *(required)* Extension identifier, e.g. `esbenp.prettier-vscode` |
| `--force` | Skip the confirmation prompt |
| `--dry-run` | Show what would be removed without making changes |

Verifies the extension is installed before attempting removal. Provides a
near-match hint if the exact ID is not found.

**Examples:**

```bash
vscode-profile-importer remove --profile "WebDev" --extension esbenp.prettier-vscode
vscode-profile-importer remove --profile Default  --extension ms-python.python --force
```

---

## uninstall

Uninstall ALL extensions from a profile.

```
vscode-profile-importer uninstall --profile <NAME> [OPTIONS]
```

| Option | Description |
|---|---|
| `--profile <NAME>` | *(required)* Profile to target |
| `--force` | Single bulk-confirmation prompt instead of per-extension prompts |
| `--dry-run` | Show what would be removed without making changes |

**Examples:**

```bash
# Interactive (prompted for each extension)
vscode-profile-importer uninstall --profile "Python Dev Hub"

# Bulk with single confirmation
vscode-profile-importer uninstall --profile "WebDev Hub" --force

# Simulate without removing
vscode-profile-importer uninstall --profile Default --force --dry-run
```

---

## How VS Code profile creation works

VS Code registers a new named profile in `storage.json` (key
`userDataProfiles`) only when a window is opened with that profile.  
This tool triggers registration by running:

```bash
setsid code --profile "$NAME" "$TMPDIR" &
```

then polling `storage.json` (1 s intervals, up to `--create-timeout` seconds)
until the profile appears, then killing the entire process group.

Once registered, VS Code assigns a short random hex hash as the profile
directory name (e.g. `-7f581919`) under `~/.config/Code/User/profiles/`.
This tool reads that hash from `storage.json` and writes `settings.json` /
`keybindings.json` there.

---

## Profile directory layout

```
~/.config/Code/User/profiles/<hash>/
  extensions.json    ← managed by VS Code (updated by --install-extension)
  settings.json      ← written by this tool from the .code-profile
  keybindings.json   ← written by this tool from the .code-profile
  globalStorage/
    state.vscdb      ← SQLite; not touched
```

---

## .code-profile format

| Field | Type | Notes |
|---|---|---|
| `name` | string | Canonical profile name (preferred over `displayName`) |
| `displayName` | string | Present in some exports |
| `icon` | string | VS Code icon identifier, optional |
| `settings` | string | JSON-encoded `{"settings": "<JSONC>"}` |
| `keybindings` | string | JSON-encoded `{"keybindings": "<JSONC>", "platform": <int>}` |
| `extensions` | string \| array | JSON-encoded array or direct array of `{identifier:{id,uuid}, displayName}` |
| `globalState` | string | Not imported (contains personal session data) |

---

## Building a .deb package

```bash
./scripts/build_deb.sh
# output: target/debian/vscode-profile-importer_*.deb
```

Requires `cargo-deb`. Installs to:

| Path | Contents |
|---|---|
| `/usr/bin/vscode-profile-importer` | CLI binary |
| `/usr/bin/install_systemd_user` | Systemd user-service helper |
| `/usr/share/vscode-profile-importer/examples/` | Example `.code-profile` files |
| `/usr/share/doc/vscode-profile-importer/` | README, INSTALL_SYSTEMD, systemd examples |
| `/usr/share/man/man1/install_systemd_user.1` | Manpage |

---

## Running tests

```bash
# Default test suite (all guarded tests skipped)
./scripts/run_tests.sh

# Packaging test only (requires ar, tar)
./scripts/run_tests.sh --packaging

# Real VS Code detection test (requires code CLI)
./scripts/run_tests.sh --real-vscode

# Everything
./scripts/run_tests.sh --all
```

---

## Known limitations

- **Requires a display for profile creation.** Set `DISPLAY` appropriately
  on headless servers, or pre-create profiles and pass `--no-use-code-cli`.
- **No `globalState` import.** The `globalState` block is intentionally
  skipped to avoid importing stale session/auth data.
- **Single file per invocation.** Batch import requires a shell loop.
