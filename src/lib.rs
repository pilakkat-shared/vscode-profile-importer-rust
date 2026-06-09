use regex::Regex;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum ImportError {
    Io(std::io::Error),
    Json(serde_json::Error),
    Invalid(String),
}

impl std::fmt::Display for ImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportError::Io(e) => write!(f, "I/O error: {}", e),
            ImportError::Json(e) => write!(f, "JSON error: {}", e),
            ImportError::Invalid(s) => write!(f, "Invalid profile: {}", s),
        }
    }
}

impl std::error::Error for ImportError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ImportError::Io(e) => Some(e),
            ImportError::Json(e) => Some(e),
            ImportError::Invalid(_) => None,
        }
    }
}

impl From<std::io::Error> for ImportError {
    fn from(e: std::io::Error) -> Self {
        ImportError::Io(e)
    }
}

impl From<serde_json::Error> for ImportError {
    fn from(e: serde_json::Error) -> Self {
        ImportError::Json(e)
    }
}

// ── Parsed representation of a .code-profile ──────────────────────────────

/// A fully-decoded, normalised representation of a `.code-profile` file.
#[derive(Debug)]
pub struct ParsedProfile {
    /// Canonical profile name (from `name` field, falling back to `displayName`).
    pub name: String,
    /// Filesystem-safe version of the name (replaces invalid chars with `_`).
    pub folder_name: String,
    /// Optional VS Code icon identifier.
    pub icon: String,
    /// Raw JSONC string for `settings.json` (may be empty).
    pub settings_content: String,
    /// Raw JSONC string for `keybindings.json` (may be empty).
    pub keybindings_content: String,
    /// List of extension IDs to install.
    pub extensions: Vec<String>,
}

// ── Public file parsing ────────────────────────────────────────────────────

/// Read and fully decode a `.code-profile` file, tolerating embedded control
/// characters and the two known extension-encoding variants (JSON string vs
/// direct array).
pub fn read_profile_file<P: AsRef<Path>>(path: P) -> Result<serde_json::Map<String, Value>, ImportError> {
    let raw = fs::read_to_string(path)?;
    parse_outer_tolerant(&raw)
}

/// High-level parse: returns a [`ParsedProfile`] ready for import.
pub fn parse_profile<P: AsRef<Path>>(path: P) -> Result<ParsedProfile, ImportError> {
    let raw = fs::read_to_string(path)?;
    let outer = parse_outer_tolerant(&raw)?;

    // Prefer 'name' over 'displayName'.
    let name = match (outer.get("name"), outer.get("displayName")) {
        (Some(Value::String(n)), _) if !n.trim().is_empty() => n.trim().to_string(),
        (_, Some(Value::String(d))) if !d.trim().is_empty() => d.trim().to_string(),
        _ => return Err(ImportError::Invalid("Profile has no 'name' field".into())),
    };

    // Filesystem-safe folder name: replace / \ : * ? " < > | with _
    let folder_name = make_folder_name(&name);

    let icon = outer.get("icon")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // settings: double-encoded JSON object {"settings": "<JSONC>"}
    let settings_content = decode_jsonc_field(&outer, "settings", "settings");

    // keybindings: double-encoded JSON object {"keybindings": "<JSONC>", "platform": ...}
    let keybindings_content = decode_jsonc_field(&outer, "keybindings", "keybindings");

    // extensions: either JSON-encoded array string OR direct array
    let extensions = extract_extension_ids(&outer);

    Ok(ParsedProfile {
        name,
        folder_name,
        icon,
        settings_content,
        keybindings_content,
        extensions,
    })
}

// ── Filesystem-safe folder name ────────────────────────────────────────────

/// Mirror of the Python: `re.sub(r'[/\\:*?"<>|]', '_', name).strip()`.
pub fn make_folder_name(s: &str) -> String {
    let out: String = s.chars().map(|c| {
        if "/\\:*?\"<>|".contains(c) { '_' } else { c }
    }).collect();
    let trimmed = out.trim().to_string();
    if trimmed.is_empty() { "imported-profile".to_string() } else { trimmed }
}

/// Legacy helper kept for tests; wraps make_folder_name for callers that used
/// the old `make_safe_basename` name.
pub fn make_safe_basename(s: &str) -> String {
    make_folder_name(s)
}

// ── storage.json helpers ───────────────────────────────────────────────────

/// Default path to VS Code's `storage.json`.
pub fn default_storage_json() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config/Code/User/globalStorage/storage.json")
}

/// Check whether a named profile is registered in `storage.json`.
pub fn profile_registered(name: &str, storage_json: &Path) -> bool {
    let Ok(data) = fs::read_to_string(storage_json) else { return false };
    let Ok(v) = serde_json::from_str::<Value>(&data) else { return false };
    v.get("userDataProfiles")
        .and_then(|p| p.as_array())
        .map(|arr| arr.iter().any(|e| e.get("name").and_then(|n| n.as_str()) == Some(name)))
        .unwrap_or(false)
}

/// Resolve the on-disk profile directory from `storage.json`.
/// VS Code records the `location` field (a short random hex string) for each
/// named profile; the actual directory is `profiles/<location>/`.
pub fn resolve_profile_dir(name: &str, storage_json: &Path) -> Option<PathBuf> {
    let data = fs::read_to_string(storage_json).ok()?;
    let v: Value = serde_json::from_str(&data).ok()?;
    let location = v.get("userDataProfiles")?
        .as_array()?
        .iter()
        .find(|e| e.get("name").and_then(|n| n.as_str()) == Some(name))?
        .get("location")?
        .as_str()?
        .to_string();

    let profiles_dir = storage_json
        .parent()? // globalStorage
        .parent()? // User
        .join("profiles");

    Some(profiles_dir.join(&location))
}

/// Patch the `icon` field of a named profile inside `storage.json`.
pub fn patch_icon(name: &str, icon: &str, storage_json: &Path) -> Result<(), ImportError> {
    let data = fs::read_to_string(storage_json)?;
    let mut v: Value = serde_json::from_str(&data)?;
    if let Some(arr) = v.get_mut("userDataProfiles").and_then(|p| p.as_array_mut()) {
        for entry in arr.iter_mut() {
            if entry.get("name").and_then(|n| n.as_str()) == Some(name) {
                entry["icon"] = Value::String(icon.to_string());
                break;
            }
        }
    }
    let out = serde_json::to_string_pretty(&v)?;
    fs::write(storage_json, out.as_bytes())?;
    Ok(())
}

// ── Import report ──────────────────────────────────────────────────────────

pub type Report = HashMap<String, Value>;

/// Core import logic (called from `main` with real closures, and from tests
/// with stubs).
///
/// The `create_profile` closure must:
///   - Launch VS Code in a new process group with the profile name
///   - Poll `storage.json` until the profile appears
///   - Kill the process group and clean up
///   - Return `Ok(())` on success or an error string
///
/// The `installer` closure receives `(profile_name, ext_id)` so it can call
/// `code --profile <name> --install-extension <id> --force`.
///
/// The `prompt_overwrite` closure is called when the profile already exists.
/// The `prompt_extension_fail` closure is called when an extension install fails.
pub fn import_profile<FCreate, FInstall, FOverwrite, FExtFail, P>(
    path: P,
    storage_json: P,
    mut create_profile: FCreate,
    mut installer: FInstall,
    mut prompt_overwrite: FOverwrite,
    mut prompt_extension_fail: FExtFail,
    report_path: Option<P>,
) -> Result<Report, ImportError>
where
    FCreate:    FnMut(&str) -> Result<(), String>,
    FInstall:   FnMut(&str, &str) -> bool, // (profile_name, ext_id)
    FOverwrite: FnMut(&str) -> String,     // "overwrite" | "cancel"
    FExtFail:   FnMut(&str) -> String,     // "skip" | "abort" | "retry"
    P: AsRef<Path>,
{
    let profile = parse_profile(path.as_ref())?;
    let storage = storage_json.as_ref();

    let mut report: Report = HashMap::new();
    report.insert("profile".to_string(), Value::String(profile.name.clone()));
    report.insert("installed".to_string(), Value::Array(vec![]));
    report.insert("skipped".to_string(), Value::Array(vec![]));
    report.insert("failed".to_string(), Value::Array(vec![]));

    // ── Step 1: create or check for existing profile ───────────────────────
    let is_default = profile.name.to_lowercase() == "default";

    if !is_default {
        if profile_registered(&profile.name, storage) {
            // Profile already exists — ask caller whether to proceed.
            let action = prompt_overwrite(&profile.name);
            if action == "cancel" {
                return Err(ImportError::Invalid("User cancelled".into()));
            }
            // "overwrite" → fall through and re-use existing profile dir.
        } else {
            create_profile(&profile.name)
                .map_err(|e| ImportError::Invalid(format!("Profile creation failed: {}", e)))?;
        }
    }

    // ── Step 2: resolve profile directory from storage.json ────────────────
    let profile_dir = if is_default {
        // Default profile uses the main User directory.
        storage.parent() // globalStorage
            .and_then(|p| p.parent()) // User
            .map(|p| p.to_path_buf())
            .ok_or_else(|| ImportError::Invalid("Cannot resolve User dir from storage.json path".into()))?
    } else {
        resolve_profile_dir(&profile.name, storage)
            .ok_or_else(|| ImportError::Invalid(
                format!("Could not find profile directory for '{}' in storage.json", profile.name)
            ))?
    };

    if !profile_dir.exists() {
        return Err(ImportError::Invalid(
            format!("Profile directory does not exist: {}", profile_dir.display())
        ));
    }

    report.insert("profile_dir".to_string(), Value::String(profile_dir.display().to_string()));

    // ── Step 3: patch icon ─────────────────────────────────────────────────
    if !profile.icon.is_empty() && !is_default {
        let _ = patch_icon(&profile.name, &profile.icon, storage);
    }

    // ── Step 4: install extensions into the named profile ──────────────────
    for ext_id in &profile.extensions {
        let ok = installer(&profile.name, ext_id);
        if ok {
            if let Some(Value::Array(v)) = report.get_mut("installed") {
                v.push(Value::String(ext_id.clone()));
            }
            continue;
        }

        loop {
            let action = prompt_extension_fail(ext_id);
            match action.as_str() {
                "skip" => {
                    if let Some(Value::Array(v)) = report.get_mut("skipped") {
                        v.push(Value::String(ext_id.clone()));
                    }
                    break;
                }
                "abort" => {
                    if let Some(Value::Array(v)) = report.get_mut("failed") {
                        v.push(Value::String(ext_id.clone()));
                    }
                    return Err(ImportError::Invalid(
                        format!("Aborted during extension install: {}", ext_id)
                    ));
                }
                "retry" => {
                    if installer(&profile.name, ext_id) {
                        if let Some(Value::Array(v)) = report.get_mut("installed") {
                            v.push(Value::String(ext_id.clone()));
                        }
                        break;
                    }
                    continue;
                }
                _ => {
                    // Unknown response → treat as skip.
                    if let Some(Value::Array(v)) = report.get_mut("skipped") {
                        v.push(Value::String(ext_id.clone()));
                    }
                    break;
                }
            }
        }
    }

    // ── Step 5: write settings.json and keybindings.json ───────────────────
    if !profile.settings_content.is_empty() {
        let dest = profile_dir.join("settings.json");
        fs::write(&dest, profile.settings_content.as_bytes())?;
        report.insert("settings_written".to_string(), Value::String(dest.display().to_string()));
    }

    if !profile.keybindings_content.is_empty() {
        let dest = profile_dir.join("keybindings.json");
        fs::write(&dest, profile.keybindings_content.as_bytes())?;
        report.insert("keybindings_written".to_string(), Value::String(dest.display().to_string()));
    }

    // ── Step 6: optional report file ───────────────────────────────────────
    if let Some(p) = report_path {
        let j = serde_json::to_string_pretty(&report)?;
        let _ = fs::write(p.as_ref(), j.as_bytes());
    }

    Ok(report)
}

// ── Internal helpers ───────────────────────────────────────────────────────

fn parse_outer_tolerant(raw: &str) -> Result<serde_json::Map<String, Value>, ImportError> {
    // First attempt: standard parse.
    if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(raw) {
        return Ok(decode_nested_strings(map));
    }

    // Second attempt: escape bare newlines inside JSON strings.
    let escaped = escape_newlines_in_strings(raw);
    if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(&escaped) {
        return Ok(decode_nested_strings(map));
    }

    // Third attempt: regex-based tolerant extraction of known top-level keys.
    let mut top = serde_json::Map::new();

    // `name` is mandatory.
    let name = extract_string_field(raw, "name")
        .or_else(|| extract_string_field(raw, "displayName"))
        .ok_or_else(|| ImportError::Invalid("Profile has no 'name' or 'displayName' field".into()))?;
    top.insert("name".to_string(), Value::String(name));

    for key in ["icon", "settings", "keybindings", "extensions", "globalState"] {
        if let Some(v) = extract_any_field(raw, key) {
            top.insert(key.to_string(), v);
        }
    }

    Ok(decode_nested_strings(top))
}

/// Try to decode known string-valued fields that are actually JSON-encoded.
fn decode_nested_strings(mut map: serde_json::Map<String, Value>) -> serde_json::Map<String, Value> {
    for k in ["settings", "keybindings", "extensions", "globalState"] {
        if let Some(Value::String(s)) = map.get(k) {
            if let Ok(v) = serde_json::from_str::<Value>(s) {
                map.insert(k.to_string(), v);
            }
        }
    }
    map
}

/// Extract the JSONC string content from a double-encoded field.
///
/// A profile's `settings` field looks like:
/// `{"settings": "{\n  \"editor.tabSize\": 4\n}"}` — a JSON object whose only
/// relevant inner value is a raw JSONC string.  We want to return that JSONC
/// string so it can be written verbatim to `settings.json`.
fn decode_jsonc_field(outer: &serde_json::Map<String, Value>, outer_key: &str, inner_key: &str) -> String {
    let val = match outer.get(outer_key) {
        Some(v) => v,
        None => return String::new(),
    };

    // May already be a decoded object (if decode_nested_strings ran first).
    let obj = match val {
        Value::Object(o) => o,
        Value::String(s) => {
            // Try to parse the string as JSON to get at the inner object.
            if let Ok(Value::Object(o)) = serde_json::from_str::<Value>(s) {
                return o.get(inner_key)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
            }
            // Already raw JSONC — return as-is.
            return s.clone();
        }
        _ => return String::new(),
    };

    obj.get(inner_key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// Extract extension IDs from the `extensions` field (handles both encoding
/// variants: JSON-string-encoded array and direct array).
fn extract_extension_ids(outer: &serde_json::Map<String, Value>) -> Vec<String> {
    let val = match outer.get("extensions") {
        Some(v) => v,
        None => return vec![],
    };

    // Resolve to an array.
    let arr: Vec<Value> = match val {
        Value::Array(a) => a.clone(),
        Value::String(s) => {
            if let Ok(Value::Array(a)) = serde_json::from_str::<Value>(s) {
                a
            } else {
                return vec![];
            }
        }
        _ => return vec![],
    };

    arr.iter().filter_map(extract_ext_id).collect()
}

fn extract_ext_id(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.trim().to_string()),
        Value::Object(map) => {
            // {identifier: {id: "...", uuid: "..."}, displayName: "..."}
            if let Some(Value::Object(ident)) = map.get("identifier") {
                if let Some(Value::String(id)) = ident.get("id") {
                    let s = id.trim().to_string();
                    if !s.is_empty() { return Some(s); }
                }
            }
            // Flat {id: "..."} variant.
            if let Some(Value::String(id)) = map.get("id") {
                let s = id.trim().to_string();
                if !s.is_empty() { return Some(s); }
            }
            None
        }
        _ => None,
    }
}

fn escape_newlines_in_strings(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_string = false;
    let mut esc = false;
    let mut chars = s.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '"' && !esc {
            in_string = !in_string;
            out.push(ch);
            esc = false;
            continue;
        }
        if ch == '\\' && !esc {
            esc = true;
            out.push(ch);
            continue;
        }
        if (ch == '\n' || ch == '\r') && in_string {
            if ch == '\r' {
                if let Some(&'\n') = chars.peek() {
                    chars.next();
                }
            }
            out.push_str("\\n");
            esc = false;
            continue;
        }
        out.push(ch);
        esc = false;
    }
    out
}

fn extract_string_field(raw: &str, key: &str) -> Option<String> {
    let re = Regex::new(&format!(r#"\"{}\"\s*:\s*"((?:[^"\\]|\\.)*)""#, regex::escape(key))).ok()?;
    let caps = re.captures(raw)?;
    Some(caps[1].to_string())
}

fn extract_any_field(raw: &str, key: &str) -> Option<Value> {
    let re = Regex::new(&format!(r#"\"{}\"\s*:\s*"#, regex::escape(key))).ok()?;
    let m = re.find(raw)?;
    let mut i = m.end();
    while i < raw.len() && raw.as_bytes()[i].is_ascii_whitespace() { i += 1; }
    if i >= raw.len() { return None; }
    let tail = &raw[i..];
    serde_json::from_str::<Value>(tail).ok()
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::tempdir;

    fn example_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("vscode-profile-importer")
            .join("contrib")
            .join("examples")
            .join(name)
    }

    // ── make_folder_name / make_safe_basename ──────────────────────────────

    #[test]
    fn test_make_folder_name_plain() {
        assert_eq!(make_folder_name("SimpleName"), "SimpleName");
    }

    #[test]
    fn test_make_folder_name_spaces_preserved() {
        assert_eq!(make_folder_name("Rust Dev Hub"), "Rust Dev Hub");
    }

    #[test]
    fn test_make_folder_name_invalid_chars() {
        assert_eq!(make_folder_name("C/C++ Dev"), "C_C++ Dev");
        assert_eq!(make_folder_name("A:B*C?D"), "A_B_C_D");
    }

    #[test]
    fn test_make_safe_basename_compat() {
        // The legacy wrapper must still work.
        assert_eq!(make_safe_basename("My Profile"), "My Profile");
        assert_eq!(make_safe_basename("C/C++ Dev"), "C_C++ Dev");
    }

    // ── parse_profile ──────────────────────────────────────────────────────

    #[test]
    fn test_parse_profile_from_file() {
        let p = example_path("example.code-profile");
        let pp = parse_profile(p).expect("parse profile");
        assert!(!pp.name.is_empty(), "name must be non-empty");
    }

    #[test]
    fn test_parse_profile_extensions_array_variant() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("a.code-profile");
        // direct array variant (no double-encoding)
        let j = serde_json::json!({
            "name": "ArrayExt",
            "extensions": [
                {"identifier": {"id": "foo.bar", "uuid": "x"}, "displayName": "Foo Bar"},
                {"identifier": {"id": "baz.qux", "uuid": "y"}, "displayName": "Baz Qux"}
            ]
        });
        fs::write(&p, serde_json::to_string(&j).unwrap()).unwrap();
        let pp = parse_profile(&p).expect("parse");
        assert_eq!(pp.name, "ArrayExt");
        assert_eq!(pp.extensions, vec!["foo.bar", "baz.qux"]);
    }

    #[test]
    fn test_parse_profile_extensions_string_variant() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("b.code-profile");
        // string-encoded array variant
        let ext_arr = r#"[{"identifier":{"id":"a.b","uuid":"u1"},"displayName":"A B"}]"#;
        let j = serde_json::json!({ "name": "StringExt", "extensions": ext_arr });
        fs::write(&p, serde_json::to_string(&j).unwrap()).unwrap();
        let pp = parse_profile(&p).expect("parse");
        assert_eq!(pp.extensions, vec!["a.b"]);
    }

    #[test]
    fn test_parse_profile_settings_double_encoded() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("c.code-profile");
        // settings is a JSON-encoded object: {"settings": "<JSONC>"}
        let inner_jsonc = r#"{"editor.tabSize": 4}"#;
        let settings_val = serde_json::to_string(&serde_json::json!({"settings": inner_jsonc})).unwrap();
        let j = serde_json::json!({ "name": "SettingsTest", "settings": settings_val });
        fs::write(&p, serde_json::to_string(&j).unwrap()).unwrap();
        let pp = parse_profile(&p).expect("parse");
        assert_eq!(pp.settings_content, inner_jsonc);
    }

    #[test]
    fn test_parse_profile_no_name_errors() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("noname.code-profile");
        fs::write(&p, br#"{"icon": "gear"}"#).unwrap();
        assert!(parse_profile(&p).is_err());
    }

    #[test]
    fn test_parse_profile_tolerant_control_chars() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("ctrl.code-profile");
        // raw newline inside a JSON string value (as VS Code sometimes exports)
        let content = "{\"name\": \"Ctrl\",\n\"settings\": \"line1\nline2\"}";
        fs::write(&p, content).unwrap();
        let pp = parse_profile(&p).expect("tolerant parse");
        assert_eq!(pp.name, "Ctrl");
    }

    // ── read_profile_file (low-level) ──────────────────────────────────────

    #[test]
    fn test_read_profile_default() {
        let p = example_path("example.code-profile");
        let m = read_profile_file(p).expect("read profile");
        assert!(m.get("name").is_some());
    }

    #[test]
    fn test_read_profile_tolerant_malformed() {
        let tmp = tempdir().unwrap();
        let p = tmp.path().join("malformed.code-profile");
        let content = "{\n  \"name\": \"Tolerant\",\n  \"settings\": \"{\n\\\"editor.tabSize\\\": 4\n}\"\n}\n";
        fs::write(&p, content.as_bytes()).unwrap();
        let m = read_profile_file(&p).expect("should parse tolerant profile");
        assert_eq!(m.get("name").and_then(|v| v.as_str()), Some("Tolerant"));
        assert!(m.get("settings").is_some());
    }

    // ── profile_registered / resolve_profile_dir / patch_icon ─────────────

    fn fake_storage(tmp: &tempfile::TempDir, name: &str, location: &str) -> PathBuf {
        let gs = tmp.path().join("globalStorage");
        fs::create_dir_all(&gs).unwrap();
        let s = serde_json::json!({
            "userDataProfiles": [
                {"name": name, "location": location, "icon": ""},
            ]
        });
        let p = gs.join("storage.json");
        fs::write(&p, serde_json::to_string_pretty(&s).unwrap()).unwrap();
        p
    }

    #[test]
    fn test_profile_registered_true() {
        let tmp = tempdir().unwrap();
        let sp = fake_storage(&tmp, "MyProfile", "-abc123");
        assert!(profile_registered("MyProfile", &sp));
        assert!(!profile_registered("Other", &sp));
    }

    #[test]
    fn test_resolve_profile_dir() {
        let tmp = tempdir().unwrap();
        let sp = fake_storage(&tmp, "MyProfile", "-abc123");
        let profiles = tmp.path().join("profiles").join("-abc123");
        fs::create_dir_all(&profiles).unwrap();
        let dir = resolve_profile_dir("MyProfile", &sp).unwrap();
        assert_eq!(dir, profiles);
    }

    #[test]
    fn test_patch_icon() {
        let tmp = tempdir().unwrap();
        let sp = fake_storage(&tmp, "MyProfile", "-abc123");
        patch_icon("MyProfile", "rocket", &sp).unwrap();
        let data: Value = serde_json::from_str(&fs::read_to_string(&sp).unwrap()).unwrap();
        let icon = data["userDataProfiles"][0]["icon"].as_str().unwrap();
        assert_eq!(icon, "rocket");
    }

    // ── import_profile ─────────────────────────────────────────────────────

    /// Build a minimal fake storage.json + profiles dir so import_profile can
    /// resolve the profile directory.
    fn setup_fake_vscode(tmp: &tempfile::TempDir, profile_name: &str) -> (PathBuf, PathBuf) {
        let location = "-fakehash";
        let profile_dir = tmp.path().join("profiles").join(location);
        fs::create_dir_all(&profile_dir).unwrap();
        let sp = fake_storage(tmp, profile_name, location);
        (sp, profile_dir)
    }

    #[test]
    fn test_import_profile_basic() {
        let tmp = tempdir().unwrap();
        let (storage, profile_dir) = setup_fake_vscode(&tmp, "TestProfile");

        let prof = tmp.path().join("p.code-profile");
        let j = serde_json::json!({ "name": "TestProfile", "extensions": [
            {"identifier": {"id": "foo.bar", "uuid": "u"}, "displayName": "Foo"}
        ]});
        fs::write(&prof, serde_json::to_string(&j).unwrap()).unwrap();

        let mut installed: Vec<(String,String)> = vec![];
        let create_profile = |_name: &str| Ok(());
        let installer = |name: &str, ext: &str| {
            installed.push((name.to_string(), ext.to_string()));
            true
        };
        let prompt_overwrite = |_: &str| "overwrite".to_string();
        let prompt_ext_fail = |_: &str| "skip".to_string();

        let report = import_profile(
            &prof, &storage, create_profile, installer,
            prompt_overwrite, prompt_ext_fail, None::<&PathBuf>,
        ).expect("import");

        assert_eq!(report["profile"].as_str(), Some("TestProfile"));
        assert_eq!(report["installed"].as_array().unwrap().len(), 1);
        assert_eq!(report["installed"][0].as_str(), Some("foo.bar"));
        // installer must be called with the profile name
        assert!(installed.iter().any(|(n,_)| n == "TestProfile"));
        // profile dir unchanged (no settings/keybindings in source)
        assert!(!profile_dir.join("settings.json").exists());
    }

    #[test]
    fn test_import_profile_writes_settings() {
        let tmp = tempdir().unwrap();
        let (storage, profile_dir) = setup_fake_vscode(&tmp, "SettingsProfile");

        let prof = tmp.path().join("s.code-profile");
        let inner_jsonc = r#"{"editor.tabSize": 2}"#;
        let settings_val = serde_json::to_string(
            &serde_json::json!({"settings": inner_jsonc})
        ).unwrap();
        let j = serde_json::json!({ "name": "SettingsProfile", "settings": settings_val });
        fs::write(&prof, serde_json::to_string(&j).unwrap()).unwrap();

        import_profile(
            &prof, &storage,
            |_| Ok(()),
            |_, _| true,
            |_| "overwrite".to_string(),
            |_| "skip".to_string(),
            None::<&PathBuf>,
        ).expect("import");

        let written = fs::read_to_string(profile_dir.join("settings.json")).unwrap();
        assert_eq!(written, inner_jsonc);
    }

    #[test]
    fn test_import_profile_cancel() {
        let tmp = tempdir().unwrap();
        // Make profile appear already registered so prompt fires.
        let (storage, _) = setup_fake_vscode(&tmp, "ExistingProfile");

        let prof = tmp.path().join("e.code-profile");
        fs::write(&prof, br#"{"name": "ExistingProfile"}"#).unwrap();

        let r = import_profile(
            &prof, &storage,
            |_| Ok(()),
            |_, _| true,
            |_| "cancel".to_string(),
            |_| "skip".to_string(),
            None::<&PathBuf>,
        );
        assert!(r.is_err());
        assert!(r.unwrap_err().to_string().contains("cancelled"));
    }

    #[test]
    fn test_import_profile_ext_abort() {
        let tmp = tempdir().unwrap();
        let (storage, _) = setup_fake_vscode(&tmp, "AbortProfile");

        let prof = tmp.path().join("ab.code-profile");
        let j = serde_json::json!({ "name": "AbortProfile", "extensions": [
            {"identifier": {"id": "bad.ext", "uuid": "u"}, "displayName": "Bad"}
        ]});
        fs::write(&prof, serde_json::to_string(&j).unwrap()).unwrap();

        let r = import_profile(
            &prof, &storage,
            |_| Ok(()),
            |_, _| false,      // installer always fails
            |_| "overwrite".to_string(),
            |_| "abort".to_string(), // abort on failure
            None::<&PathBuf>,
        );
        assert!(r.is_err());
    }

    #[test]
    fn test_import_profile_ext_retry_then_succeed() {
        let tmp = tempdir().unwrap();
        let (storage, _) = setup_fake_vscode(&tmp, "RetryProfile");

        let prof = tmp.path().join("r.code-profile");
        let j = serde_json::json!({ "name": "RetryProfile", "extensions": [
            {"identifier": {"id": "retry.ext", "uuid": "u"}, "displayName": "Retry"}
        ]});
        fs::write(&prof, serde_json::to_string(&j).unwrap()).unwrap();

        let mut calls = 0usize;
        let r = import_profile(
            &prof, &storage,
            |_| Ok(()),
            move |_, _| { calls += 1; calls > 1 }, // fail first, succeed second
            |_| "overwrite".to_string(),
            |_| "retry".to_string(),
            None::<&PathBuf>,
        );
        assert!(r.is_ok());
        assert_eq!(r.unwrap()["installed"][0].as_str(), Some("retry.ext"));
    }

    #[test]
    fn test_import_profile_ext_installer_receives_profile_name() {
        let tmp = tempdir().unwrap();
        let (storage, _) = setup_fake_vscode(&tmp, "NamedProfile");

        let prof = tmp.path().join("np.code-profile");
        let j = serde_json::json!({ "name": "NamedProfile", "extensions": [
            {"identifier": {"id": "x.y", "uuid": "u"}, "displayName": "XY"}
        ]});
        fs::write(&prof, serde_json::to_string(&j).unwrap()).unwrap();

        let mut seen_name = String::new();
        let mut seen_ext = String::new();
        import_profile(
            &prof, &storage,
            |_| Ok(()),
            |name, ext| { seen_name = name.to_string(); seen_ext = ext.to_string(); true },
            |_| "overwrite".to_string(),
            |_| "skip".to_string(),
            None::<&PathBuf>,
        ).unwrap();
        assert_eq!(seen_name, "NamedProfile");
        assert_eq!(seen_ext, "x.y");
    }

    // ── escape_newlines_in_strings ─────────────────────────────────────────

    #[test]
    fn test_escape_newlines_in_strings() {
        let raw = "{\"a\": \"line1\nline2\"}";
        let out = escape_newlines_in_strings(raw);
        assert!(out.contains("\\n"));
    }

    // ── extract_ext_id ─────────────────────────────────────────────────────

    #[test]
    fn test_extract_ext_id_varieties() {
        use serde_json::json;
        assert_eq!(extract_ext_id(&Value::String("a.b".into())).as_deref(), Some("a.b"));
        assert_eq!(extract_ext_id(&json!({"identifier": {"id": "x.y"}})).as_deref(), Some("x.y"));
        assert_eq!(extract_ext_id(&json!({"id": "z"})).as_deref(), Some("z"));
        assert_eq!(extract_ext_id(&json!(null)), None);
    }
}
