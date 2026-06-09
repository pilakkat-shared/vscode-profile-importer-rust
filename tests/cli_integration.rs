use assert_cmd::prelude::*;
use std::process::Command;
use tempfile::tempdir;
use std::fs;
use vscode_profile_importer::make_safe_basename;

/// Build a minimal fake VS Code environment:
///  - storage.json with the profile already registered (so dry-run can succeed)
///  - profile dir on disk
fn fake_vscode_env(tmp: &tempfile::TempDir, profile_name: &str) -> std::path::PathBuf {
    let location = "-fakehash";
    let profile_dir = tmp.path().join("profiles").join(location);
    fs::create_dir_all(&profile_dir).unwrap();
    let gs = tmp.path().join("globalStorage");
    fs::create_dir_all(&gs).unwrap();
    let s = serde_json::json!({
        "userDataProfiles": [{"name": profile_name, "location": location}]
    });
    let sp = gs.join("storage.json");
    fs::write(&sp, serde_json::to_string_pretty(&s).unwrap()).unwrap();
    sp
}

// ── CLI dry-run ────────────────────────────────────────────────────────────

#[test]
fn cli_dry_run_prints_imported() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let prof = tmp.path().join("cli.code-profile");
    let j = serde_json::json!({ "name": "CliTest", "extensions": [
        {"identifier": {"id": "a.b", "uuid": "u"}, "displayName": "AB"}
    ]});
    fs::write(&prof, serde_json::to_string(&j)?)?;

    // Provide a fake storage.json so the tool can resolve the profile dir.
    let storage = fake_vscode_env(&tmp, "CliTest");

    let mut cmd = Command::cargo_bin("vscode-profile-importer")?;
    cmd.arg("import")
        .arg(prof.to_str().unwrap())
        .arg("--storage-json").arg(storage.to_str().unwrap())
        .arg("--dry-run")
        .arg("--non-interactive")
        .arg("--no-use-code-cli");

    cmd.assert().success().stdout(predicates::str::contains("Imported profile"));
    Ok(())
}

// ── make_safe_basename / make_folder_name ──────────────────────────────────

#[test]
fn cli_folder_name_sanitization() {
    // Slash is replaced with underscore, spaces kept
    assert_eq!(make_safe_basename("C/C++ Dev"), "C_C++ Dev");
    // Snow character → underscore-based fallback
    assert!(make_safe_basename("Weird / Name: ☃").starts_with("Weird"));
}

// ── profile_registered round-trip ─────────────────────────────────────────

#[test]
fn profile_registered_detects_existing() {
    use vscode_profile_importer::profile_registered;
    let tmp = tempdir().unwrap();
    let sp = fake_vscode_env(&tmp, "DetectMe");
    assert!(profile_registered("DetectMe", &sp));
    assert!(!profile_registered("NotThere", &sp));
}

// ── resolve_profile_dir round-trip ────────────────────────────────────────

#[test]
fn resolve_profile_dir_finds_hash_dir() {
    use vscode_profile_importer::resolve_profile_dir;
    let tmp = tempdir().unwrap();
    let sp = fake_vscode_env(&tmp, "HashDir");
    let dir = resolve_profile_dir("HashDir", &sp).unwrap();
    assert!(dir.exists(), "profile dir should exist on disk");
    assert!(dir.to_str().unwrap().ends_with("-fakehash"));
}

// ── import_profile writes settings.json into hashed dir ───────────────────

#[test]
fn import_writes_settings_to_hashed_dir() -> Result<(), Box<dyn std::error::Error>> {
    use vscode_profile_importer::import_profile;
    let tmp = tempdir()?;
    let storage = fake_vscode_env(&tmp, "WithSettings");

    let inner_jsonc = r#"{"editor.tabSize": 4}"#;
    let settings_val = serde_json::to_string(
        &serde_json::json!({"settings": inner_jsonc})
    )?;
    let prof = tmp.path().join("ws.code-profile");
    let j = serde_json::json!({ "name": "WithSettings", "settings": settings_val });
    fs::write(&prof, serde_json::to_string(&j)?)?;

    import_profile(
        &prof, &storage,
        |_| Ok(()),
        |_, _| true,
        |_| "overwrite".to_string(),
        |_| "skip".to_string(),
        None::<&std::path::PathBuf>,
    )?;

    let profile_dir = tmp.path().join("profiles").join("-fakehash");
    let written = fs::read_to_string(profile_dir.join("settings.json"))?;
    assert_eq!(written, inner_jsonc);
    Ok(())
}

// ── extension installer receives (profile_name, ext_id) ───────────────────

#[test]
fn ext_installer_receives_profile_name_and_id() -> Result<(), Box<dyn std::error::Error>> {
    use vscode_profile_importer::import_profile;
    let tmp = tempdir()?;
    let storage = fake_vscode_env(&tmp, "ExtNameCheck");

    let prof = tmp.path().join("enc.code-profile");
    let j = serde_json::json!({ "name": "ExtNameCheck", "extensions": [
        {"identifier": {"id": "pub.ext", "uuid": "u"}, "displayName": "Pub"}
    ]});
    fs::write(&prof, serde_json::to_string(&j)?)?;

    let mut seen = Vec::<(String, String)>::new();
    import_profile(
        &prof, &storage,
        |_| Ok(()),
        |name, ext| { seen.push((name.to_string(), ext.to_string())); true },
        |_| "overwrite".to_string(),
        |_| "skip".to_string(),
        None::<&std::path::PathBuf>,
    )?;

    assert_eq!(seen, vec![("ExtNameCheck".to_string(), "pub.ext".to_string())]);
    Ok(())
}
