#!/usr/bin/env bash
# vscode-profile-extensions.sh
# Manage extensions for VSCode profiles.
#
# Usage:
#   ./vscode-profile-extensions.sh list
#   ./vscode-profile-extensions.sh uninstall --profile <name>
#   ./vscode-profile-extensions.sh uninstall --profile <name> --force
#   ./vscode-profile-extensions.sh uninstall --profile <name> --dry-run

set -euo pipefail

# ---------------------------------------------------------------------------
# Constants
# ---------------------------------------------------------------------------
STORAGE_JSON="${HOME}/.config/Code/User/globalStorage/storage.json"
PROFILES_DIR="${HOME}/.config/Code/User/profiles"
SCRIPT_NAME="$(basename "$0")"

# ---------------------------------------------------------------------------
# Colours (disabled automatically when not a terminal)
# ---------------------------------------------------------------------------
if [[ -t 1 ]]; then
    C_RESET='\033[0m'
    C_BOLD='\033[1m'
    C_RED='\033[0;31m'
    C_YELLOW='\033[0;33m'
    C_GREEN='\033[0;32m'
    C_CYAN='\033[0;36m'
    C_DIM='\033[2m'
else
    C_RESET='' C_BOLD='' C_RED='' C_YELLOW='' C_GREEN='' C_CYAN='' C_DIM=''
fi

# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------
die()   { printf "${C_RED}ERROR:${C_RESET} $*\n" >&2; exit 1; }
info()  { printf "${C_CYAN}>>>${C_RESET} %s\n" "$*"; }
ok()    { printf "${C_GREEN}OK${C_RESET}  %s\n" "$*"; }
warn()  { printf "${C_YELLOW}WARN:${C_RESET} %s\n" "$*"; }
hr()    { printf '%0.s─' $(seq 1 "${COLUMNS:-72}"); printf '\n'; }

require_cmd() {
    command -v "$1" &>/dev/null || die "Required command not found: $1"
}

require_cmd code
require_cmd python3

# ---------------------------------------------------------------------------
# Check VSCode storage file exists
# ---------------------------------------------------------------------------
[[ -f "$STORAGE_JSON" ]] || die "VSCode storage file not found: $STORAGE_JSON"

# ---------------------------------------------------------------------------
# get_profiles: emit JSON array of {name, location} from storage.json,
# then add the implicit Default profile.
# ---------------------------------------------------------------------------
get_profiles_json() {
    python3 - <<'PYEOF'
import json, sys, os

storage_path = os.path.expanduser("~/.config/Code/User/globalStorage/storage.json")
profiles_dir = os.path.expanduser("~/.config/Code/User/profiles")

with open(storage_path) as f:
    data = json.load(f)

results = []

# Built-in "Default" profile (no entry in userDataProfiles; extensions.json is
# at the User/ root level).
default_ext = os.path.expanduser("~/.config/Code/User/extensions.json")
results.append({
    "name": "Default",
    "location": "__default__",
    "icon": None,
    "ext_count": len(json.load(open(default_ext))) if os.path.exists(default_ext) else -1,
    "builtin": True
})

for p in data.get("userDataProfiles", []):
    loc = p.get("location", "")
    name = p.get("name", "")
    icon = p.get("icon")

    # Skip the built-in Agents profile (read-only, no extensions to manage)
    if loc == "builtin/agents":
        continue

    ext_file = os.path.join(profiles_dir, loc, "extensions.json")
    count = -1
    if os.path.exists(ext_file):
        try:
            count = len(json.load(open(ext_file)))
        except Exception:
            pass

    results.append({
        "name": name,
        "location": loc,
        "icon": icon,
        "ext_count": count,
        "builtin": False
    })

print(json.dumps(results))
PYEOF
}

# ---------------------------------------------------------------------------
# cmd_list: list all profiles with extension counts
# ---------------------------------------------------------------------------
cmd_list() {
    info "VSCode profiles for user ${USER}:"
    hr

    local profiles_json
    profiles_json="$(get_profiles_json)"

    python3 - "$profiles_json" <<'PYEOF'
import json, sys

profiles = json.loads(sys.argv[1])
name_width = max((len(p["name"]) for p in profiles), default=10)
fmt = "  {:<{w}}  {:>5}  {}"

print(fmt.format("PROFILE NAME", "EXTS", "ICON", w=name_width))
print("  " + "-" * (name_width + 14))
for p in profiles:
    count = str(p["ext_count"]) if p["ext_count"] >= 0 else "?"
    icon  = p["icon"] or ""
    tag   = "  [default]" if p["name"] == "Default" else ""
    print(fmt.format(p["name"], count, icon + tag, w=name_width))
PYEOF
    printf '\n'
    info "Use '${SCRIPT_NAME} uninstall --profile <name>' to remove extensions."
}

# ---------------------------------------------------------------------------
# get_extensions_for_profile <profile_name>
# Prints one extension-id per line using `code --profile ... --list-extensions`.
# For "Default" profile, omits --profile flag.
# ---------------------------------------------------------------------------
get_extensions_for_profile() {
    local profile_name="$1"
    if [[ "$profile_name" == "Default" ]]; then
        code --list-extensions 2>/dev/null
    else
        code --profile "$profile_name" --list-extensions 2>/dev/null
    fi
}

# ---------------------------------------------------------------------------
# profile_exists <name>: returns 0 if the profile is known, 1 otherwise
# ---------------------------------------------------------------------------
profile_exists() {
    local target="$1"
    local profiles_json
    profiles_json="$(get_profiles_json)"
    python3 - "$profiles_json" "$target" <<'PYEOF'
import json, sys
profiles = json.loads(sys.argv[1])
target   = sys.argv[2]
sys.exit(0 if any(p["name"] == target for p in profiles) else 1)
PYEOF
}

# ---------------------------------------------------------------------------
# confirm_yes_no <prompt>: returns 0 for y/Y, 1 otherwise
# ---------------------------------------------------------------------------
confirm_yes_no() {
    local prompt="$1"
    local reply
    printf "${C_BOLD}%s${C_RESET} [y/N] " "$prompt"
    read -r reply
    [[ "$reply" =~ ^[Yy]$ ]]
}

# ---------------------------------------------------------------------------
# cmd_extensions <profile_name>: list extensions installed in a profile
# ---------------------------------------------------------------------------
cmd_extensions() {
    local profile_name="$1"

    profile_exists "$profile_name" \
        || die "Profile not found: '${profile_name}'\n       Run '${SCRIPT_NAME} list' to see available profiles."

    local ext_list
    ext_list="$(get_extensions_for_profile "$profile_name")"

    local ext_count
    ext_count="$(printf '%s\n' "$ext_list" | grep -c '[^[:space:]]' || true)"

    info "Extensions in profile '${profile_name}' (${ext_count}):"
    hr
    if [[ "$ext_count" -eq 0 ]]; then
        printf '  (none)\n'
    else
        local idx=0
        while IFS= read -r ext; do
            [[ -z "$ext" ]] && continue
            (( idx++ )) || true
            printf "  %3d.  %s\n" "$idx" "$ext"
        done <<< "$ext_list"
    fi
    hr
}

# ---------------------------------------------------------------------------
# do_uninstall_one <profile_name> <ext_id>
# Shared helper: runs the actual `code --uninstall-extension` call.
# ---------------------------------------------------------------------------
do_uninstall_one() {
    local profile_name="$1" ext="$2"
    if [[ "$profile_name" == "Default" ]]; then
        code --uninstall-extension "$ext" &>/dev/null
    else
        code --profile "$profile_name" --uninstall-extension "$ext" &>/dev/null
    fi
}

# ---------------------------------------------------------------------------
# cmd_remove <profile_name> <ext_id>
# Uninstall a single named extension from a profile, with confirmation.
# ---------------------------------------------------------------------------
cmd_remove() {
    local profile_name="$1"
    local ext_id="$2"

    profile_exists "$profile_name" \
        || die "Profile not found: '${profile_name}'\n       Run '${SCRIPT_NAME} list' to see available profiles."

    # Verify the extension is actually installed in this profile
    local ext_list
    ext_list="$(get_extensions_for_profile "$profile_name")"

    local match
    match="$(printf '%s\n' "$ext_list" | grep -i "^${ext_id}$" || true)"

    if [[ -z "$match" ]]; then
        # Try a case-insensitive partial hint for a better error message
        local hint
        hint="$(printf '%s\n' "$ext_list" | grep -i "$ext_id" || true)"
        if [[ -n "$hint" ]]; then
            die "Extension '${ext_id}' not found in profile '${profile_name}'.\n       Did you mean one of:\n$(printf '%s\n' "$hint" | sed 's/^/         /')"
        else
            die "Extension '${ext_id}' is not installed in profile '${profile_name}'."
        fi
    fi

    # Use the exact id as reported by VSCode (preserves original casing)
    ext_id="$match"

    printf "${C_BOLD}Profile:${C_RESET}    %s\n" "$profile_name"
    printf "${C_BOLD}Extension:${C_RESET}  %s\n\n" "$ext_id"

    confirm_yes_no "Uninstall this extension?" || { info "Aborted."; exit 0; }

    printf "Uninstalling..."
    if do_uninstall_one "$profile_name" "$ext_id"; then
        printf " ${C_GREEN}done${C_RESET}\n"
        ok "Removed '${ext_id}' from profile '${profile_name}'."
    else
        printf " ${C_RED}FAILED${C_RESET}\n"
        die "Uninstall command returned a non-zero exit code."
    fi
}

# ---------------------------------------------------------------------------
# cmd_uninstall <profile_name> <force_flag> <dry_run_flag>
# ---------------------------------------------------------------------------
cmd_uninstall() {
    local profile_name="$1"
    local force="${2:-false}"        # true = bulk, no per-extension prompt
    local dry_run="${3:-false}"      # true = simulate only

    # --- Validate profile -----------------------------------------------
    profile_exists "$profile_name" \
        || die "Profile not found: '${profile_name}'\n       Run '${SCRIPT_NAME} list' to see available profiles."

    # --- Gather extensions -----------------------------------------------
    info "Fetching extensions for profile '${profile_name}'..."
    local ext_list
    ext_list="$(get_extensions_for_profile "$profile_name")"

    local ext_count
    ext_count="$(printf '%s\n' "$ext_list" | grep -c '[^[:space:]]' || true)"

    if [[ "$ext_count" -eq 0 ]]; then
        info "No extensions installed in profile '${profile_name}'. Nothing to do."
        exit 0
    fi

    printf '\n'
    printf "${C_BOLD}Profile:${C_RESET}     %s\n" "$profile_name"
    printf "${C_BOLD}Extensions:${C_RESET}  %d\n" "$ext_count"
    [[ "$dry_run" == "true" ]] && printf "${C_YELLOW}Mode:        DRY RUN (no changes will be made)${C_RESET}\n"
    printf '\n'

    # --- Bulk mode: single upfront confirmation --------------------------
    if [[ "$force" == "true" ]]; then
        printf "${C_YELLOW}WARNING:${C_RESET} This will uninstall ALL %d extensions from profile '${profile_name}'.\n" "$ext_count"
        printf '\n'
        printf '%s\n' "$ext_list" | while read -r ext; do
            printf "  ${C_DIM}-${C_RESET} %s\n" "$ext"
        done
        printf '\n'

        if [[ "$dry_run" != "true" ]]; then
            confirm_yes_no "Confirm bulk removal of all ${ext_count} extensions?" \
                || { info "Aborted. No extensions were removed."; exit 0; }
        fi
        printf '\n'

        local removed=0 failed=0
        while IFS= read -r ext; do
            [[ -z "$ext" ]] && continue
            if [[ "$dry_run" == "true" ]]; then
                printf "  ${C_DIM}[dry-run]${C_RESET} would uninstall: %s\n" "$ext"
                (( removed++ )) || true
            else
                printf "  Uninstalling ${C_BOLD}%s${C_RESET}..." "$ext"
                if [[ "$profile_name" == "Default" ]]; then
                    code --uninstall-extension "$ext" &>/dev/null && { printf " ${C_GREEN}done${C_RESET}\n"; (( removed++ )) || true; } \
                        || { printf " ${C_RED}FAILED${C_RESET}\n"; (( failed++ )) || true; }
                else
                    code --profile "$profile_name" --uninstall-extension "$ext" &>/dev/null \
                        && { printf " ${C_GREEN}done${C_RESET}\n"; (( removed++ )) || true; } \
                        || { printf " ${C_RED}FAILED${C_RESET}\n"; (( failed++ )) || true; }
                fi
            fi
        done <<< "$ext_list"

        printf '\n'
        hr
        if [[ "$dry_run" == "true" ]]; then
            ok "Dry run complete. ${removed} extension(s) would be removed."
        else
            ok "Done. Removed: ${removed}  Failed: ${failed}"
        fi
        return
    fi

    # --- Interactive mode: confirm each extension one at a time ----------
    info "You will be prompted for each extension. Press Ctrl+C to abort at any time."
    printf '\n'

    local idx=0 removed=0 skipped=0 failed=0
    # Read the extension list on fd 3 so that stdin (fd 0) stays connected to
    # the terminal and confirm_yes_no's `read` can receive keystrokes.
    while IFS= read -r ext <&3; do
        [[ -z "$ext" ]] && continue
        (( idx++ )) || true
        printf "${C_BOLD}[%d/%d]${C_RESET} %s\n" "$idx" "$ext_count" "$ext"

        if [[ "$dry_run" == "true" ]]; then
            printf "  ${C_DIM}[dry-run]${C_RESET} would uninstall.\n\n"
            (( removed++ )) || true
            continue
        fi

        if confirm_yes_no "  Uninstall this extension?"; then
            printf "  Uninstalling..."
            if [[ "$profile_name" == "Default" ]]; then
                code --uninstall-extension "$ext" &>/dev/null \
                    && { printf " ${C_GREEN}done${C_RESET}\n\n"; (( removed++ )) || true; } \
                    || { printf " ${C_RED}FAILED${C_RESET}\n\n"; (( failed++ )) || true; }
            else
                code --profile "$profile_name" --uninstall-extension "$ext" &>/dev/null \
                    && { printf " ${C_GREEN}done${C_RESET}\n\n"; (( removed++ )) || true; } \
                    || { printf " ${C_RED}FAILED${C_RESET}\n\n"; (( failed++ )) || true; }
            fi
        else
            printf "  Skipped.\n\n"
            (( skipped++ )) || true
        fi
    done 3<<< "$ext_list"

    hr
    if [[ "$dry_run" == "true" ]]; then
        ok "Dry run complete. ${removed} extension(s) would be removed."
    else
        ok "Done. Removed: ${removed}  Skipped: ${skipped}  Failed: ${failed}"
    fi
}

# ---------------------------------------------------------------------------
# Usage / help
# ---------------------------------------------------------------------------
usage() {
    cat <<EOF
${C_BOLD}Usage:${C_RESET}
  ${SCRIPT_NAME} list
  ${SCRIPT_NAME} extensions --profile <name>
  ${SCRIPT_NAME} remove --profile <name> --extension <ext-id>
  ${SCRIPT_NAME} uninstall --profile <name> [--force] [--dry-run]

${C_BOLD}Commands:${C_RESET}
  list                     List all VSCode profiles and their extension counts.
  extensions               List extensions installed in a specific profile.
  remove                   Uninstall a single extension from a profile.
  uninstall                Uninstall ALL extensions from a profile.

${C_BOLD}Options for extensions / remove / uninstall:${C_RESET}
  --profile <name>         Profile to target (required). Use quotes for names
                           containing spaces, e.g. --profile "C/C++ Dev Hub".

${C_BOLD}Additional option for remove:${C_RESET}
  --extension <ext-id>     Extension identifier to remove, e.g.
                           --extension esbenp.prettier-vscode (required).

${C_BOLD}Additional options for uninstall:${C_RESET}
  --force                  Skip per-extension prompts; ask once for bulk
                           confirmation before removing all extensions.
  --dry-run                Show what would be removed without making changes.

${C_BOLD}Examples:${C_RESET}
  ${SCRIPT_NAME} list
  ${SCRIPT_NAME} extensions --profile "Python Dev Hub"
  ${SCRIPT_NAME} remove --profile "Node.js" --extension dbaeumer.vscode-eslint
  ${SCRIPT_NAME} uninstall --profile "Python Dev Hub"
  ${SCRIPT_NAME} uninstall --profile "WebDev Hub" --force
  ${SCRIPT_NAME} uninstall --profile Default --force --dry-run
EOF
}

# ---------------------------------------------------------------------------
# Argument parsing
# ---------------------------------------------------------------------------
COMMAND="${1:-}"
shift || true

case "$COMMAND" in
    list)
        cmd_list
        ;;

    extensions)
        PROFILE_NAME=""

        while [[ $# -gt 0 ]]; do
            case "$1" in
                --profile)
                    [[ $# -ge 2 ]] || die "--profile requires an argument."
                    PROFILE_NAME="$2"
                    shift 2
                    ;;
                *)
                    die "Unknown option: $1\n\n$(usage)"
                    ;;
            esac
        done

        [[ -n "$PROFILE_NAME" ]] || die "--profile <name> is required for the extensions command."

        cmd_extensions "$PROFILE_NAME"
        ;;

    remove)
        PROFILE_NAME=""
        EXT_ID=""

        while [[ $# -gt 0 ]]; do
            case "$1" in
                --profile)
                    [[ $# -ge 2 ]] || die "--profile requires an argument."
                    PROFILE_NAME="$2"
                    shift 2
                    ;;
                --extension)
                    [[ $# -ge 2 ]] || die "--extension requires an argument."
                    EXT_ID="$2"
                    shift 2
                    ;;
                *)
                    die "Unknown option: $1\n\n$(usage)"
                    ;;
            esac
        done

        [[ -n "$PROFILE_NAME" ]] || die "--profile <name> is required for the remove command."
        [[ -n "$EXT_ID" ]]       || die "--extension <ext-id> is required for the remove command."

        cmd_remove "$PROFILE_NAME" "$EXT_ID"
        ;;

    uninstall)
        PROFILE_NAME=""
        FORCE="false"
        DRY_RUN="false"

        while [[ $# -gt 0 ]]; do
            case "$1" in
                --profile)
                    [[ $# -ge 2 ]] || die "--profile requires an argument."
                    PROFILE_NAME="$2"
                    shift 2
                    ;;
                --force)
                    FORCE="true"
                    shift
                    ;;
                --dry-run)
                    DRY_RUN="true"
                    shift
                    ;;
                *)
                    die "Unknown option: $1\n\n$(usage)"
                    ;;
            esac
        done

        [[ -n "$PROFILE_NAME" ]] || die "--profile <name> is required for the uninstall command."

        cmd_uninstall "$PROFILE_NAME" "$FORCE" "$DRY_RUN"
        ;;

    help|--help|-h)
        usage
        ;;

    "")
        usage
        exit 1
        ;;

    *)
        die "Unknown command: '${COMMAND}'\n\n$(usage)"
        ;;
esac
