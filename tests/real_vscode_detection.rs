use std::error::Error;
use std::fs;
use std::path::PathBuf;
use tempfile::tempdir;
use vscode_profile_importer::{import_profile, profile_registered, resolve_profile_dir};

// ── helpers ────────────────────────────────────────────────────────────────

fn fake_storage(tmp: &tempfile::TempDir, name: &str, location: &str) -> PathBuf {
    let gs = tmp.path().join("globalStorage");
    fs::create_dir_all(&gs).unwrap();
    let profile_dir = tmp.path().join("profiles").join(location);
    fs::create_dir_all(&profile_dir).unwrap();
    let s = serde_json::json!({
        "userDataProfiles": [{"name": name, "location": location}]
    });
    let p = gs.join("storage.json");
    fs::write(&p, serde_json::to_string_pretty(&s).unwrap()).unwrap();
    p
}

// ── Verify that after importing a profile the hashed directory exists on
//    disk and contains settings/keybindings if they were in the source. ──────

#[test]
fn profile_discovery_after_import() -> Result<(), Box<dyn Error>> {
    let tmp = tempdir()?;
    let storage = fake_storage(&tmp, "VSCodeDetectTest", "-detesthash");

    let prof = tmp.path().join("real.code-profile");
    let name = "VSCodeDetectTest";
    let j = serde_json::json!({ "name": name, "extensions": [] });
    fs::write(&prof, serde_json::to_string(&j)?)?;

    let report = import_profile(
        &prof, &storage,
        |_| Ok(()),
        |_, _| true,
        |_| "overwrite".to_string(),
        |_| "skip".to_string(),
        None::<&PathBuf>,
    )?;

    assert!(report.get("installed").is_some());

    // The profile must be registered in storage.json.
    assert!(profile_registered(name, &storage), "profile should be registered");

    // The hashed profile directory must exist.
    let dir = resolve_profile_dir(name, &storage)
        .expect("must resolve profile dir");
    assert!(dir.exists(), "profile dir must exist: {}", dir.display());

    Ok(())
}

#[test]
fn profile_discovery_with_settings_written() -> Result<(), Box<dyn Error>> {
    let tmp = tempdir()?;
    let storage = fake_storage(&tmp, "WithSettingsProfile", "-sethash");

    let inner_jsonc = r#"{"editor.fontSize": 14}"#;
    let settings_val = serde_json::to_string(
        &serde_json::json!({"settings": inner_jsonc})
    )?;
    let prof = tmp.path().join("ws.code-profile");
    let j = serde_json::json!({ "name": "WithSettingsProfile", "settings": settings_val });
    fs::write(&prof, serde_json::to_string(&j)?)?;

    import_profile(
        &prof, &storage,
        |_| Ok(()),
        |_, _| true,
        |_| "overwrite".to_string(),
        |_| "skip".to_string(),
        None::<&PathBuf>,
    )?;

    let dir = resolve_profile_dir("WithSettingsProfile", &storage).unwrap();
    let settings = fs::read_to_string(dir.join("settings.json"))?;
    assert_eq!(settings, inner_jsonc);
    Ok(())
}
