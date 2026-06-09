// Integration tests for the management subcommands:
//   list, extensions, remove, uninstall
//
// All tests use a fake `code` binary on PATH to avoid needing a real VS Code
// installation and to make assertions about exactly which CLI commands are
// invoked.

use assert_cmd::prelude::*;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use tempfile::tempdir;

// ── Test helpers ───────────────────────────────────────────────────────────

/// Build a minimal VS Code environment under `tmp`:
///   - storage.json with named profiles
///   - extensions.json per profile
///   - a fake `code` binary that logs its argv and can pretend to
///     list/uninstall extensions
struct FakeVscode {
    pub storage_json: PathBuf,
    pub bin_dir: PathBuf,
    pub log: PathBuf,
}

impl FakeVscode {
    /// Create the environment.
    /// `profiles`: list of (name, location, extensions) tuples.
    fn new(tmp: &tempfile::TempDir, profiles: &[(&str, &str, &[&str])]) -> Self {
        let gs = tmp.path().join("globalStorage");
        fs::create_dir_all(&gs).unwrap();

        let mut entries = vec![];
        for (name, loc, exts) in profiles {
            let pd = tmp.path().join("profiles").join(loc);
            fs::create_dir_all(&pd).unwrap();
            // extensions.json (used by list_profiles for counts)
            let ext_json: Vec<serde_json::Value> = exts.iter()
                .map(|id| serde_json::json!({"id": id}))
                .collect();
            fs::write(pd.join("extensions.json"),
                serde_json::to_string(&ext_json).unwrap()).unwrap();

            entries.push(serde_json::json!({"name": name, "location": loc}));
        }

        let storage_json = gs.join("storage.json");
        let s = serde_json::json!({"userDataProfiles": entries});
        fs::write(&storage_json, serde_json::to_string_pretty(&s).unwrap()).unwrap();

        // Fake `code` binary
        let bin_dir = tmp.path().join("bin");
        fs::create_dir_all(&bin_dir).unwrap();
        let log = tmp.path().join("code_calls.log");
        let code_path = bin_dir.join("code");
        let log_str = log.to_str().unwrap().to_string();

        // The script logs all args, then handles specific flags:
        //   --list-extensions → print each extension id on its own line
        //   --uninstall-extension <id> → log and exit 0
        //   --version → echo fake version
        //
        // Extension list is stored as a space-separated list in CODE_EXTENSIONS env var.
        let script = format!(
            r#"#!/usr/bin/env bash
echo "$@" >> {log}
if [[ "$*" == *"--version"* ]]; then echo "fake 1.0"; exit 0; fi
if [[ "$*" == *"--list-extensions"* ]]; then
  IFS=' ' read -ra EXTS <<< "${{CODE_EXTENSIONS:-}}"
  for e in "${{EXTS[@]}}"; do echo "$e"; done
  exit 0
fi
if [[ "$*" == *"--uninstall-extension"* ]]; then exit 0; fi
if [[ "$*" == *"--install-extension"* ]]; then exit 0; fi
exit 0
"#,
            log = log_str
        );
        fs::write(&code_path, script.as_bytes()).unwrap();
        use std::os::unix::fs::PermissionsExt;
        let mut p = fs::metadata(&code_path).unwrap().permissions();
        p.set_mode(0o755);
        fs::set_permissions(&code_path, p).unwrap();

        FakeVscode { storage_json, bin_dir, log }
    }

    fn path_env(&self) -> String {
        format!(
            "{}:{}",
            self.bin_dir.to_str().unwrap(),
            std::env::var("PATH").unwrap_or_default()
        )
    }

    fn log_contents(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }
}

// ── list ───────────────────────────────────────────────────────────────────

#[test]
fn cli_list_shows_profiles() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let vsc = FakeVscode::new(&tmp, &[
        ("WebDev",  "-aaa", &["esbenp.prettier-vscode", "dbaeumer.vscode-eslint"]),
        ("RustDev", "-bbb", &["rust-lang.rust-analyzer"]),
    ]);

    let mut cmd = Command::cargo_bin("vscode-profile-importer")?;
    cmd.args(["list", "--storage-json", vsc.storage_json.to_str().unwrap()]);
    let out = cmd.assert().success().get_output().stdout.clone();
    let stdout = String::from_utf8_lossy(&out);

    assert!(stdout.contains("WebDev"),  "should list WebDev: {}", stdout);
    assert!(stdout.contains("RustDev"), "should list RustDev: {}", stdout);
    assert!(stdout.contains("Default"), "should list Default: {}", stdout);
    // Extension counts
    assert!(stdout.contains('2'), "WebDev should show 2 extensions: {}", stdout);
    assert!(stdout.contains('1'), "RustDev should show 1 extension: {}", stdout);
    Ok(())
}

#[test]
fn cli_list_empty_storage() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let vsc = FakeVscode::new(&tmp, &[]);

    let mut cmd = Command::cargo_bin("vscode-profile-importer")?;
    cmd.args(["list", "--storage-json", vsc.storage_json.to_str().unwrap()]);
    let out = cmd.assert().success().get_output().stdout.clone();
    let stdout = String::from_utf8_lossy(&out);
    // Should still show Default
    assert!(stdout.contains("Default"), "{}", stdout);
    Ok(())
}

// ── extensions ─────────────────────────────────────────────────────────────

#[test]
fn cli_extensions_lists_installed() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let vsc = FakeVscode::new(&tmp, &[
        ("MyProfile", "-ccc", &[]),
    ]);

    let mut cmd = Command::cargo_bin("vscode-profile-importer")?;
    cmd.args(["extensions", "--profile", "MyProfile",
              "--storage-json", vsc.storage_json.to_str().unwrap()])
        .env("PATH", vsc.path_env())
        .env("CODE_EXTENSIONS", "foo.bar baz.qux"); // fake code will print these

    let out = cmd.assert().success().get_output().stdout.clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(stdout.contains("foo.bar"), "{}", stdout);
    assert!(stdout.contains("baz.qux"), "{}", stdout);

    // Verify the CLI called: code --profile MyProfile --list-extensions
    let log = vsc.log_contents();
    assert!(log.contains("--profile") && log.contains("MyProfile"), "{}", log);
    assert!(log.contains("--list-extensions"), "{}", log);
    Ok(())
}

#[test]
fn cli_extensions_unknown_profile_errors() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let vsc = FakeVscode::new(&tmp, &[]);

    let mut cmd = Command::cargo_bin("vscode-profile-importer")?;
    cmd.args(["extensions", "--profile", "NonExistent",
              "--storage-json", vsc.storage_json.to_str().unwrap()])
        .env("PATH", vsc.path_env());

    cmd.assert().failure();
    Ok(())
}

// ── remove ─────────────────────────────────────────────────────────────────

#[test]
fn cli_remove_dry_run() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let vsc = FakeVscode::new(&tmp, &[("RemoveProfile", "-ddd", &[])]);

    let mut cmd = Command::cargo_bin("vscode-profile-importer")?;
    cmd.args(["remove",
              "--profile",   "RemoveProfile",
              "--extension", "esbenp.prettier-vscode",
              "--force",
              "--dry-run",
              "--storage-json", vsc.storage_json.to_str().unwrap()])
        .env("PATH", vsc.path_env())
        // Fake code reports this extension as installed
        .env("CODE_EXTENSIONS", "esbenp.prettier-vscode");

    let out = cmd.assert().success().get_output().stdout.clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(stdout.contains("[dry-run]"), "{}", stdout);

    // No uninstall call should appear in the log
    let log = vsc.log_contents();
    assert!(!log.contains("--uninstall-extension"), "should not uninstall in dry-run: {}", log);
    Ok(())
}

#[test]
fn cli_remove_calls_uninstall_extension() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let vsc = FakeVscode::new(&tmp, &[("UninstP", "-eee", &[])]);

    let mut cmd = Command::cargo_bin("vscode-profile-importer")?;
    cmd.args(["remove",
              "--profile",   "UninstP",
              "--extension", "foo.bar",
              "--force",
              "--storage-json", vsc.storage_json.to_str().unwrap()])
        .env("PATH", vsc.path_env())
        .env("CODE_EXTENSIONS", "foo.bar");

    cmd.assert().success();

    let log = vsc.log_contents();
    assert!(log.contains("--uninstall-extension") && log.contains("foo.bar"),
        "expected uninstall call in log: {}", log);
    assert!(log.contains("--profile") && log.contains("UninstP"),
        "expected --profile UninstP in log: {}", log);
    Ok(())
}

#[test]
fn cli_remove_unknown_extension_errors() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let vsc = FakeVscode::new(&tmp, &[("P", "-fff", &[])]);

    let mut cmd = Command::cargo_bin("vscode-profile-importer")?;
    cmd.args(["remove",
              "--profile", "P", "--extension", "not.installed",
              "--force",
              "--storage-json", vsc.storage_json.to_str().unwrap()])
        .env("PATH", vsc.path_env())
        .env("CODE_EXTENSIONS", "foo.bar"); // different extension installed

    cmd.assert().failure();
    Ok(())
}

// ── uninstall ──────────────────────────────────────────────────────────────

#[test]
fn cli_uninstall_dry_run_force() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let vsc = FakeVscode::new(&tmp, &[("BulkP", "-ggg", &[])]);

    let mut cmd = Command::cargo_bin("vscode-profile-importer")?;
    cmd.args(["uninstall",
              "--profile", "BulkP",
              "--force", "--dry-run",
              "--storage-json", vsc.storage_json.to_str().unwrap()])
        .env("PATH", vsc.path_env())
        .env("CODE_EXTENSIONS", "a.b c.d");

    let out = cmd.assert().success().get_output().stdout.clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(stdout.contains("[dry-run]"), "{}", stdout);
    assert!(stdout.contains("a.b"), "{}", stdout);
    assert!(stdout.contains("c.d"), "{}", stdout);

    // Nothing should actually be uninstalled
    let log = vsc.log_contents();
    assert!(!log.contains("--uninstall-extension"), "no real uninstall in dry-run: {}", log);
    Ok(())
}

#[test]
fn cli_uninstall_force_calls_uninstall_for_each() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let vsc = FakeVscode::new(&tmp, &[("BulkReal", "-hhh", &[])]);

    // Prepopulate the confirmation by piping 'y' to stdin
    let mut cmd = std::process::Command::new(
        assert_cmd::cargo::cargo_bin("vscode-profile-importer")
    );
    cmd.args(["uninstall",
              "--profile", "BulkReal",
              "--force",
              "--storage-json", vsc.storage_json.to_str().unwrap()])
        .env("PATH", vsc.path_env())
        .env("CODE_EXTENSIONS", "ext.one ext.two")
        .stdin(Stdio::piped());

    let mut child = cmd.spawn()?;
    // write 'y' to confirm bulk removal
    use std::io::Write;
    if let Some(stdin) = child.stdin.as_mut() {
        let _ = stdin.write_all(b"y\n");
    }
    let status = child.wait()?;
    assert!(status.success(), "uninstall command failed");

    let log = vsc.log_contents();
    assert!(log.contains("ext.one"),  "ext.one should be uninstalled: {}", log);
    assert!(log.contains("ext.two"),  "ext.two should be uninstalled: {}", log);
    assert!(log.contains("--profile") && log.contains("BulkReal"),
        "--profile BulkReal should appear: {}", log);
    Ok(())
}

#[test]
fn cli_uninstall_no_extensions_exits_cleanly() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let vsc = FakeVscode::new(&tmp, &[("EmptyP", "-iii", &[])]);

    let mut cmd = Command::cargo_bin("vscode-profile-importer")?;
    cmd.args(["uninstall", "--profile", "EmptyP",
              "--force",
              "--storage-json", vsc.storage_json.to_str().unwrap()])
        .env("PATH", vsc.path_env())
        .env("CODE_EXTENSIONS", ""); // no extensions installed

    let out = cmd.assert().success().get_output().stdout.clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(stdout.contains("Nothing to do") || stdout.contains("No extensions"), "{}", stdout);
    Ok(())
}

// ── import subcommand (smoke test) ─────────────────────────────────────────

#[test]
fn cli_import_dry_run() -> Result<(), Box<dyn std::error::Error>> {
    let tmp = tempdir()?;
    let vsc = FakeVscode::new(&tmp, &[("DryImport", "-jjj", &[])]);

    let prof = tmp.path().join("test.code-profile");
    let j = serde_json::json!({ "name": "DryImport", "extensions": [
        {"identifier": {"id": "foo.bar", "uuid": "u"}, "displayName": "Foo"}
    ]});
    fs::write(&prof, serde_json::to_string(&j)?)?;

    let mut cmd = Command::cargo_bin("vscode-profile-importer")?;
    cmd.args(["import",
              prof.to_str().unwrap(),
              "--storage-json", vsc.storage_json.to_str().unwrap(),
              "--dry-run", "--non-interactive", "--no-use-code-cli"])
        .env("PATH", vsc.path_env());

    let out = cmd.assert().success().get_output().stdout.clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(stdout.contains("Imported profile") || stdout.contains("dry-run"), "{}", stdout);
    Ok(())
}
