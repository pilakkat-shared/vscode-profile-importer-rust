use assert_cmd::prelude::*;
use std::process::Command;
use tempfile::tempdir;
use std::fs;

// Test that when a `code` CLI is present the importer calls it with
// --profile <name> --install-extension <id> --force.
#[test]
fn importer_uses_code_cli_for_ext_install() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let bin = tmp.path().join("bin");
    fs::create_dir_all(&bin)?;

    // Build a fake storage.json so the importer can resolve the profile dir
    // (profile already registered — no need to create it).
    let location = "-fakeclitest";
    let profile_dir = tmp.path().join("profiles").join(location);
    fs::create_dir_all(&profile_dir)?;
    let gs = tmp.path().join("globalStorage");
    fs::create_dir_all(&gs)?;
    let storage_json = gs.join("storage.json");
    let s = serde_json::json!({
        "userDataProfiles": [{"name": "CodeCliProfile", "location": location}]
    });
    fs::write(&storage_json, serde_json::to_string_pretty(&s)?)?;

    // Log file to capture what the fake code CLI was called with.
    let log_file = tmp.path().join("code_calls.log");

    // Create a fake `code` script.
    let code_path = bin.join("code");
    let log_path_str = log_file.to_str().unwrap().to_string();
    let script = format!(r#"#!/usr/bin/env bash
echo "$@" >> {log}
if [[ "$1" == "--version" ]]; then echo "fake 1.0.0"; exit 0; fi
exit 0
"#, log = log_path_str);
    fs::write(&code_path, script.as_bytes())?;
    use std::os::unix::fs::PermissionsExt;
    let mut perms = fs::metadata(&code_path)?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&code_path, perms)?;

    // Create profile file with one extension.
    let prof = tmp.path().join("p.code-profile");
    let j = serde_json::json!({ "name": "CodeCliProfile", "extensions": [
        {"identifier": {"id": "foo.bar", "uuid": "u"}, "displayName": "Foo Bar"}
    ]});
    fs::write(&prof, serde_json::to_string(&j)?)?;

    // Run the importer with fake code on PATH.
    let path_env = format!("{}:{}", bin.to_str().unwrap(), std::env::var("PATH").unwrap_or_default());
    let mut cmd = Command::cargo_bin("vscode-profile-importer")?;
    cmd.arg("import")
        .arg(prof.to_str().unwrap())
        .arg("--storage-json").arg(storage_json.to_str().unwrap())
        .arg("--non-interactive")
        .arg("--no-use-code-cli") // profile already exists; skip creation
        .env("PATH", path_env);

    cmd.assert().success();

    // The fake code must have been called with:
    //   --profile CodeCliProfile --install-extension foo.bar --force
    let log = fs::read_to_string(&log_file)?;
    assert!(
        log.contains("--profile") && log.contains("CodeCliProfile"),
        "expected --profile CodeCliProfile in log: {}", log
    );
    assert!(
        log.contains("--install-extension") && log.contains("foo.bar"),
        "expected --install-extension foo.bar in log: {}", log
    );
    assert!(
        log.contains("--force"),
        "expected --force in log: {}", log
    );

    Ok(())
}
