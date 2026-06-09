use std::error::Error;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

/// Packaging integration test.
///
/// This test is skipped by default. To run it, set PACKAGING_TEST=1 in the
/// environment. The test will attempt to locate a .deb under target/debian and
/// will run ./scripts/build_deb.sh if none is found. It then extracts the
/// package and verifies a few installed files exist (examples and docs).
#[test]
fn packaging_contains_examples_and_docs() -> Result<(), Box<dyn Error>> {
    if std::env::var_os("PACKAGING_TEST").is_none() {
        eprintln!("PACKAGING_TEST not set: skipping packaging integration test");
        return Ok(());
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_debian = manifest.join("target").join("debian");

    // find .deb
    let deb = fs::read_dir(&target_debian)
        .ok()
        .and_then(|rd| {
            rd.filter_map(Result::ok)
                .map(|e| e.path())
                .find(|p| p.extension().and_then(|s| s.to_str()) == Some("deb"))
        });

    let deb_path = if let Some(d) = deb {
        d
    } else {
        // try to run build script
        let script = manifest.join("scripts").join("build_deb.sh");
        if !script.exists() {
            return Err(format!("No .deb found and build script missing: {}", script.display()).into());
        }
        let status = Command::new("bash").arg(script).status()?;
        if !status.success() {
            return Err("build_deb.sh failed".into());
        }

        // re-scan
        fs::read_dir(&target_debian)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .find(|p| p.extension().and_then(|s| s.to_str()) == Some("deb"))
            .ok_or_else(|| -> Box<dyn std::error::Error> { From::from(".deb not found after build") })?
    };

    // Extract .deb using ar -> data.tar.* then tar extract
    let tmp = tempfile::tempdir()?;
    let tmpdir = tmp.path();

    // run `ar x <deb>` in tmpdir
    let ar_status = Command::new("ar").arg("x").arg(&deb_path).current_dir(&tmpdir).status();
    if ar_status.is_err() || !ar_status.unwrap().success() {
        return Err("`ar` command failed or not available; ensure binutils/ar is installed".into());
    }

    // find data.tar*
    let mut data_tar: Option<PathBuf> = None;
    for e in fs::read_dir(&tmpdir)? {
        let p = e?.path();
        if let Some(name) = p.file_name().and_then(|s| s.to_str()) {
            if name.starts_with("data.tar") {
                data_tar = Some(p);
                break;
            }
        }
    }
    let data_tar = data_tar.ok_or_else(|| "data.tar.* not found inside .deb" )?;

    let extract_dir = tmpdir.join("extracted");
    fs::create_dir_all(&extract_dir)?;

    // choose tar flags based on suffix
    let data_name = data_tar.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let tar_status = if data_name.ends_with(".gz") {
        Command::new("tar").args(&["-xzf", data_tar.to_str().unwrap(), "-C", extract_dir.to_str().unwrap()]).status()
    } else if data_name.ends_with(".xz") {
        Command::new("tar").args(&["-xJf", data_tar.to_str().unwrap(), "-C", extract_dir.to_str().unwrap()]).status()
    } else {
        Command::new("tar").args(&["-xf", data_tar.to_str().unwrap(), "-C", extract_dir.to_str().unwrap()]).status()
    };

    if tar_status.is_err() || !tar_status.unwrap().success() {
        return Err("tar extraction failed or not available".into());
    }

    // verify expected files
    let doc = extract_dir.join("usr/share/doc/vscode-profile-importer/README_EXAMPLES.md");
    let example = extract_dir.join("usr/share/vscode-profile-importer/examples/example.code-profile");
    let man = extract_dir.join("usr/share/man/man1/install_systemd_user.1");

    if !doc.exists() {
        return Err(format!("expected doc file missing: {}", doc.display()).into());
    }
    if !example.exists() {
        return Err(format!("expected example file missing: {}", example.display()).into());
    }
    if !man.exists() {
        return Err(format!("expected man file missing: {}", man.display()).into());
    }

    Ok(())
}
