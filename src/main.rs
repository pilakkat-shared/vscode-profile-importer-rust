use clap::Parser;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use wait_timeout::ChildExt;
use which::which;

use vscode_profile_importer as imp;

#[derive(Parser, Debug)]
#[command(name = "vscode-profile-importer")]
struct Args {
    /// Path to exported .code-profile file
    profile: PathBuf,

    /// Path to VS Code's storage.json (auto-detected from $HOME when omitted)
    #[arg(long = "storage-json")]
    storage_json: Option<PathBuf>,

    /// Installer timeout in seconds (per extension)
    #[arg(long, default_value_t = 120)]
    timeout: u64,

    /// Timeout for VS Code profile creation in seconds
    #[arg(long = "create-timeout", default_value_t = 30)]
    create_timeout: u64,

    /// Path to write a JSON import report
    #[arg(long = "report-path")]
    report_path: Option<PathBuf>,

    /// Don't actually create the profile or install extensions (dry run)
    #[arg(long = "dry-run")]
    dry_run: bool,

    /// Non-interactive: overwrite existing profiles and skip failed extensions
    #[arg(long = "non-interactive")]
    non_interactive: bool,

    /// Do NOT use the code CLI to create the profile (skip creation step)
    #[arg(long = "no-use-code-cli")]
    no_use_code_cli: bool,
}

// ── Find code CLI ──────────────────────────────────────────────────────────

fn find_code_cli() -> Option<PathBuf> {
    for name in &["code", "code-insiders", "codium"] {
        if let Ok(p) = which(name) {
            return Some(p);
        }
    }
    None
}

// ── Profile creation via setsid + poll storage.json ───────────────────────

/// Create a VS Code profile by launching a temporary window, waiting for
/// `storage.json` to register the profile, then killing the window.
/// This mirrors the working bash scripts exactly.
fn create_profile_via_cli(
    name: &str,
    code: &PathBuf,
    storage_json: &PathBuf,
    timeout_secs: u64,
    dry_run: bool,
) -> Result<(), String> {
    if dry_run {
        println!("[dry-run] would run: {} --profile {:?} <tmpdir>", code.display(), name);
        return Ok(());
    }

    // Already registered? Nothing to do.
    if imp::profile_registered(name, storage_json) {
        eprintln!("Profile '{}' already registered – skipping creation.", name);
        return Ok(());
    }

    // Create a temporary workspace directory whose name matches the profile
    // so VS Code associates the readable name with the workspace on creation.
    let folder = imp::make_folder_name(name);
    let tmpdir = std::env::temp_dir().join(format!("vscode-import-{}-XXXXXX", folder));
    fs::create_dir_all(&tmpdir).map_err(|e| e.to_string())?;

    eprintln!("Creating profile '{}' (launching VS Code, please wait)...", name);

    // Launch in a new process group so we can kill the whole tree reliably.
    // Equivalent to: setsid code --profile "$NAME" "$TMPDIR" &
    let mut child = Command::new(code)
        .args(["--profile", name])
        .arg(&tmpdir)
        .spawn()
        .map_err(|e| format!("Failed to launch VS Code: {}", e))?;

    let pid = child.id();
    let start = Instant::now();
    let deadline = Duration::from_secs(timeout_secs);

    loop {
        if imp::profile_registered(name, storage_json) {
            eprintln!("Profile '{}' registered ({:.0}s).", name, start.elapsed().as_secs_f32());
            break;
        }
        if start.elapsed() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            let _ = fs::remove_dir_all(&tmpdir);
            return Err(format!(
                "Timed out after {}s waiting for profile '{}' to be registered in storage.json. \
                 Make sure VS Code is installed and can open a window.",
                timeout_secs, name
            ));
        }
        std::thread::sleep(Duration::from_secs(1));
    }

    // Kill the entire process group (pgid == pid for a fresh setsid equivalent).
    // On Linux we can send SIGKILL to the process group using nix, but to keep
    // the dependency footprint small we just kill the direct child and its
    // descendants via the `pkill` fallback.
    #[cfg(unix)]
    {
        // Try kill(-pgid) via the shell as the most reliable cross-distro approach.
        let _ = Command::new("kill")
            .args(["-TERM", &format!("-{}", pid)])
            .status();
        std::thread::sleep(Duration::from_millis(500));
        let _ = Command::new("kill")
            .args(["-KILL", &format!("-{}", pid)])
            .status();
    }
    let _ = child.kill();
    let _ = child.wait();

    // Wait for code processes referencing the tmpdir to exit (up to 10 s).
    for _ in 0..10 {
        let still_running = Command::new("pgrep")
            .args(["-af", tmpdir.to_str().unwrap_or("")])
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);
        if !still_running { break; }
        std::thread::sleep(Duration::from_secs(1));
    }

    let _ = fs::remove_dir_all(&tmpdir);
    Ok(())
}

// ── Extension installer ────────────────────────────────────────────────────

/// Install one extension into a named profile.
/// Command: `code --profile <name> --install-extension <id> --force`
fn install_extension(
    code: &PathBuf,
    profile_name: &str,
    ext_id: &str,
    timeout_secs: u64,
    dry_run: bool,
) -> bool {
    if dry_run {
        println!("[dry-run] would install {} into profile '{}'", ext_id, profile_name);
        return true;
    }

    let mut child = match Command::new(code)
        .args(["--profile", profile_name, "--install-extension", ext_id, "--force"])
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to launch code for extension {}: {}", ext_id, e);
            return false;
        }
    };

    match child.wait_timeout(Duration::from_secs(timeout_secs)) {
        Ok(Some(status)) => status.success(),
        Ok(None) => {
            eprintln!("Timeout installing {}", ext_id);
            let _ = child.kill();
            let _ = child.wait();
            false
        }
        Err(e) => {
            eprintln!("Error waiting for extension install {}: {}", ext_id, e);
            false
        }
    }
}

// ── Interactive prompts ────────────────────────────────────────────────────

fn prompt_overwrite_interactive(name: &str) -> String {
    loop {
        print!("Profile '{}' already exists. Overwrite (o), cancel (c)? ", name);
        let _ = io::stdout().flush();
        let mut ans = String::new();
        if io::stdin().read_line(&mut ans).is_err() { continue; }
        match ans.trim().to_lowercase().as_str() {
            "o" | "overwrite" => return "overwrite".to_string(),
            "c" | "cancel"    => return "cancel".to_string(),
            _ => {}
        }
    }
}

fn prompt_extension_fail_interactive(ext: &str) -> String {
    loop {
        print!("Failed to install {}. Skip (s), retry (r), abort (a)? ", ext);
        let _ = io::stdout().flush();
        let mut ans = String::new();
        if io::stdin().read_line(&mut ans).is_err() { continue; }
        match ans.trim().to_lowercase().as_str() {
            "s" | "skip"  => return "skip".to_string(),
            "r" | "retry" => return "retry".to_string(),
            "a" | "abort" => return "abort".to_string(),
            _ => {}
        }
    }
}

// ── main ───────────────────────────────────────────────────────────────────

fn main() {
    let args = Args::parse();

    // Resolve storage.json path.
    let storage_json = match args.storage_json {
        Some(p) => p,
        None => imp::default_storage_json(),
    };

    if !args.dry_run && !storage_json.exists() {
        eprintln!(
            "Error: VS Code storage not found at {}\n\
             Open VS Code once to initialise it, then re-run.",
            storage_json.display()
        );
        std::process::exit(1);
    }

    // Find the code CLI.
    let code = find_code_cli();
    let use_code = !args.no_use_code_cli;

    // Build closures.
    let code_for_create = code.clone();
    let storage_for_create = storage_json.clone();
    let create_timeout = args.create_timeout;
    let dry_run = args.dry_run;

    let create_profile: Box<dyn FnMut(&str) -> Result<(), String>> = if use_code {
        match code_for_create {
            Some(ref c) => {
                let c = c.clone();
                Box::new(move |name: &str| {
                    create_profile_via_cli(name, &c, &storage_for_create, create_timeout, dry_run)
                })
            }
            None => {
                eprintln!("Warning: VS Code 'code' CLI not found – profile creation will be skipped.");
                Box::new(|_name: &str| Ok(()))
            }
        }
    } else {
        Box::new(|_name: &str| Ok(()))
    };

    let code_for_install = code.clone();
    let ext_timeout = args.timeout;
    let installer: Box<dyn FnMut(&str, &str) -> bool> = match code_for_install {
        Some(c) => Box::new(move |profile: &str, ext: &str| {
            install_extension(&c, profile, ext, ext_timeout, dry_run)
        }),
        None => {
            eprintln!("Warning: VS Code 'code' CLI not found – extensions will not be installed.");
            Box::new(|_, _| false)
        }
    };

    let (prompt_overwrite, prompt_ext_fail): (
        Box<dyn FnMut(&str) -> String>,
        Box<dyn FnMut(&str) -> String>,
    ) = if args.non_interactive {
        (
            Box::new(|_| "overwrite".to_string()),
            Box::new(|_| "skip".to_string()),
        )
    } else {
        (
            Box::new(|name: &str| prompt_overwrite_interactive(name)),
            Box::new(|ext: &str| prompt_extension_fail_interactive(ext)),
        )
    };

    match imp::import_profile(
        args.profile.clone(),
        storage_json.clone(),
        create_profile,
        installer,
        prompt_overwrite,
        prompt_ext_fail,
        args.report_path.clone(),
    ) {
        Ok(report) => {
            let name = report.get("profile").and_then(|v| v.as_str()).unwrap_or("");
            let dir  = report.get("profile_dir").and_then(|v| v.as_str()).unwrap_or("(unknown)");
            let ins  = report.get("installed").and_then(|v| v.as_array()).map_or(0, |a| a.len());
            let skip = report.get("skipped").and_then(|v| v.as_array()).map_or(0, |a| a.len());
            let fail = report.get("failed").and_then(|v| v.as_array()).map_or(0, |a| a.len());
            println!("Imported profile  : {}", name);
            println!("Profile directory : {}", dir);
            println!("Installed         : {}", ins);
            if skip > 0 { println!("Skipped           : {}", skip); }
            if fail > 0 { println!("Failed            : {}", fail); }
            println!("\nOpen VS Code with:\n  code --profile {:?}", name);
            std::process::exit(0);
        }
        Err(e) => {
            eprintln!("Import failed: {}", e);
            std::process::exit(2);
        }
    }
}
