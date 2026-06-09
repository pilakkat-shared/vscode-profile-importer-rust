//! # vscode-profile-importer
//!
//! A CLI tool to import VS Code `.code-profile` files and manage profile
//! extensions from the command line.
//!
//! ## Subcommands
//!
//! | Subcommand   | Description                                              |
//! |---|---|
//! | `import`     | Import a `.code-profile` file into VS Code               |
//! | `list`       | List all registered VS Code profiles with extension counts|
//! | `extensions` | List extensions installed in a specific profile          |
//! | `remove`     | Uninstall a single extension from a profile              |
//! | `uninstall`  | Uninstall ALL extensions from a profile                  |

use clap::{Args, Parser, Subcommand};
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::process::Command;
use std::time::{Duration, Instant};
use wait_timeout::ChildExt;
use which::which;

use vscode_profile_importer as imp;

// ── Top-level CLI definition ───────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "vscode-profile-importer",
    version,
    about = "Import VS Code profiles and manage profile extensions",
    long_about = "Import exported VS Code .code-profile files into a new or existing profile,\n\
                  and manage (list/remove/uninstall) extensions across profiles.\n\n\
                  Profile creation uses the VS Code CLI and polls storage.json until the\n\
                  profile is registered — exactly as the reference bash scripts do."
)]
struct Cli {
    #[command(subcommand)]
    command: SubCmd,
}

#[derive(Subcommand)]
enum SubCmd {
    /// Import a .code-profile file into VS Code.
    ///
    /// Creates the named profile via `code --profile <name> <tmpdir>`, waits
    /// for VS Code to register it in storage.json, then installs each extension
    /// with `code --profile <name> --install-extension <id> --force` and writes
    /// settings.json / keybindings.json into the profile's hashed directory.
    Import(ImportArgs),

    /// List all registered VS Code profiles with their extension counts.
    ///
    /// Reads the profile list from storage.json and the extension count from
    /// each profile's extensions.json file.
    List(ListArgs),

    /// List extensions installed in a specific profile.
    ///
    /// Uses `code [--profile <name>] --list-extensions` to retrieve the live
    /// list from VS Code.
    Extensions(ExtensionsArgs),

    /// Uninstall a single extension from a profile.
    ///
    /// Verifies the extension is installed before removing it, and asks for
    /// confirmation unless --force is given.
    Remove(RemoveArgs),

    /// Uninstall ALL extensions from a profile.
    ///
    /// In interactive mode you are prompted per extension; with --force you
    /// get a single bulk-confirmation prompt. Use --dry-run to preview.
    Uninstall(UninstallArgs),
}

// ── Subcommand argument structs ────────────────────────────────────────────

#[derive(Args)]
struct ImportArgs {
    /// Path to the exported .code-profile file to import.
    profile: PathBuf,

    /// Path to VS Code's storage.json (auto-detected from $HOME when omitted).
    ///
    /// Default: ~/.config/Code/User/globalStorage/storage.json
    #[arg(long = "storage-json", value_name = "PATH")]
    storage_json: Option<PathBuf>,

    /// Per-extension install timeout in seconds.
    #[arg(long, default_value_t = 120, value_name = "SECS")]
    timeout: u64,

    /// Timeout for VS Code profile creation in seconds.
    #[arg(long = "create-timeout", default_value_t = 30, value_name = "SECS")]
    create_timeout: u64,

    /// Write a JSON summary of the import to this file.
    #[arg(long = "report-path", value_name = "PATH")]
    report_path: Option<PathBuf>,

    /// Dry run: print what would happen without creating the profile or
    /// installing extensions.
    #[arg(long = "dry-run")]
    dry_run: bool,

    /// Non-interactive: overwrite existing profiles automatically and skip
    /// failed extensions without prompting.
    #[arg(long = "non-interactive")]
    non_interactive: bool,

    /// Do NOT use the code CLI to create the profile.
    ///
    /// Useful when the profile already exists or in headless environments where
    /// VS Code cannot open a window.
    #[arg(long = "no-use-code-cli")]
    no_use_code_cli: bool,
}

#[derive(Args)]
struct ListArgs {
    /// Path to VS Code's storage.json (auto-detected from $HOME when omitted).
    #[arg(long = "storage-json", value_name = "PATH")]
    storage_json: Option<PathBuf>,
}

#[derive(Args)]
struct ExtensionsArgs {
    /// Profile name to list extensions for.
    ///
    /// Use quotes for names containing spaces, e.g. --profile "C/C++ Dev Hub".
    /// Use "Default" for the built-in default profile.
    #[arg(long, value_name = "NAME")]
    profile: String,

    /// Path to VS Code's storage.json (auto-detected from $HOME when omitted).
    #[arg(long = "storage-json", value_name = "PATH")]
    storage_json: Option<PathBuf>,
}

#[derive(Args)]
struct RemoveArgs {
    /// Profile name to remove an extension from.
    #[arg(long, value_name = "NAME")]
    profile: String,

    /// Extension identifier to remove, e.g. esbenp.prettier-vscode.
    #[arg(long, value_name = "EXT-ID")]
    extension: String,

    /// Skip the confirmation prompt and uninstall immediately.
    #[arg(long)]
    force: bool,

    /// Dry run: show what would be removed without making changes.
    #[arg(long = "dry-run")]
    dry_run: bool,

    /// Path to VS Code's storage.json (auto-detected from $HOME when omitted).
    #[arg(long = "storage-json", value_name = "PATH")]
    storage_json: Option<PathBuf>,
}

#[derive(Args)]
struct UninstallArgs {
    /// Profile name to uninstall all extensions from.
    #[arg(long, value_name = "NAME")]
    profile: String,

    /// Skip per-extension prompts and ask only once for bulk confirmation.
    #[arg(long)]
    force: bool,

    /// Dry run: show what would be removed without making changes.
    #[arg(long = "dry-run")]
    dry_run: bool,

    /// Path to VS Code's storage.json (auto-detected from $HOME when omitted).
    #[arg(long = "storage-json", value_name = "PATH")]
    storage_json: Option<PathBuf>,
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

fn require_code_cli() -> PathBuf {
    find_code_cli().unwrap_or_else(|| {
        eprintln!("Error: 'code' CLI not found in PATH. Install VS Code and ensure 'code' is available.");
        std::process::exit(1);
    })
}

// ── Shared storage.json resolution ────────────────────────────────────────

fn resolve_storage(opt: Option<PathBuf>) -> PathBuf {
    opt.unwrap_or_else(imp::default_storage_json)
}

fn check_storage(storage: &PathBuf) {
    if !storage.exists() {
        eprintln!(
            "Error: VS Code storage not found at {}\n\
             Open VS Code once to initialise it, then re-run.",
            storage.display()
        );
        std::process::exit(1);
    }
}

// ── Interactive prompts ────────────────────────────────────────────────────

fn confirm(prompt: &str) -> bool {
    print!("{} [y/N] ", prompt);
    let _ = io::stdout().flush();
    let mut ans = String::new();
    if io::stdin().read_line(&mut ans).is_err() {
        return false;
    }
    matches!(ans.trim().to_lowercase().as_str(), "y" | "yes")
}

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

fn prompt_ext_fail_interactive(ext: &str) -> String {
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

// ── Profile creation via setsid + poll storage.json ───────────────────────

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

    if imp::profile_registered(name, storage_json) {
        eprintln!("Profile '{}' already registered – skipping creation.", name);
        return Ok(());
    }

    let folder = imp::make_folder_name(name);
    let tmpdir = std::env::temp_dir().join(format!("vscode-import-{}", folder));
    fs::create_dir_all(&tmpdir).map_err(|e| e.to_string())?;

    eprintln!("Creating profile '{}' (launching VS Code, please wait)...", name);

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
                "Timed out after {}s waiting for profile '{}' to appear in storage.json.",
                timeout_secs, name
            ));
        }
        std::thread::sleep(Duration::from_secs(1));
    }

    #[cfg(unix)]
    {
        let _ = Command::new("kill").args(["-TERM", &format!("-{}", pid)]).status();
        std::thread::sleep(Duration::from_millis(500));
        let _ = Command::new("kill").args(["-KILL", &format!("-{}", pid)]).status();
    }
    let _ = child.kill();
    let _ = child.wait();

    for _ in 0..10 {
        let still = Command::new("pgrep")
            .args(["-af", tmpdir.to_str().unwrap_or("")])
            .output()
            .map(|o| !o.stdout.is_empty())
            .unwrap_or(false);
        if !still { break; }
        std::thread::sleep(Duration::from_secs(1));
    }

    let _ = fs::remove_dir_all(&tmpdir);
    Ok(())
}

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
        Err(e) => { eprintln!("Failed to launch code for {}: {}", ext_id, e); return false; }
    };

    match child.wait_timeout(Duration::from_secs(timeout_secs)) {
        Ok(Some(s)) => s.success(),
        Ok(None) => { let _ = child.kill(); let _ = child.wait(); false }
        Err(_) => false,
    }
}

// ── cmd_import ─────────────────────────────────────────────────────────────

fn cmd_import(args: ImportArgs) {
    let storage = resolve_storage(args.storage_json);
    if !args.dry_run { check_storage(&storage); }

    let code = find_code_cli();
    let use_code = !args.no_use_code_cli;

    // Build create_profile closure
    let code_c = code.clone();
    let storage_c = storage.clone();
    let ct = args.create_timeout;
    let dr = args.dry_run;
    let create_profile: Box<dyn FnMut(&str) -> Result<(), String>> = if use_code {
        match code_c {
            Some(c) => Box::new(move |n| create_profile_via_cli(n, &c, &storage_c, ct, dr)),
            None => {
                eprintln!("Warning: 'code' CLI not found – profile creation skipped.");
                Box::new(|_| Ok(()))
            }
        }
    } else {
        Box::new(|_| Ok(()))
    };

    // Build installer closure
    let code_i = code.clone();
    let timeout = args.timeout;
    let installer: Box<dyn FnMut(&str, &str) -> bool> = match code_i {
        Some(c) => Box::new(move |p, e| install_extension(&c, p, e, timeout, dr)),
        None => {
            eprintln!("Warning: 'code' CLI not found – extensions will not be installed.");
            Box::new(|_, _| false)
        }
    };

    let (prompt_overwrite, prompt_ext_fail): (
        Box<dyn FnMut(&str) -> String>,
        Box<dyn FnMut(&str) -> String>,
    ) = if args.non_interactive {
        (Box::new(|_| "overwrite".to_string()), Box::new(|_| "skip".to_string()))
    } else {
        (
            Box::new(|n: &str| prompt_overwrite_interactive(n)),
            Box::new(|e: &str| prompt_ext_fail_interactive(e)),
        )
    };

    match imp::import_profile(
        args.profile.clone(), storage.clone(),
        create_profile, installer,
        prompt_overwrite, prompt_ext_fail,
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
        }
        Err(e) => { eprintln!("Import failed: {}", e); std::process::exit(2); }
    }
}

// ── cmd_list ───────────────────────────────────────────────────────────────

fn cmd_list(args: ListArgs) {
    let storage = resolve_storage(args.storage_json);
    check_storage(&storage);

    match imp::list_profiles(&storage) {
        Ok(profiles) => {
            let name_w = profiles.iter().map(|p| p.name.len()).max().unwrap_or(12).max(12);
            println!("  {:<name_w$}  {:>5}  {}", "PROFILE NAME", "EXTS", "ICON", name_w = name_w);
            println!("  {}", "-".repeat(name_w + 14));
            for p in &profiles {
                let count = p.ext_count.map_or("?".to_string(), |c| c.to_string());
                let icon  = p.icon.as_deref().unwrap_or("");
                let tag   = if p.is_default { "  [default]" } else { "" };
                println!("  {:<name_w$}  {:>5}  {}{}", p.name, count, icon, tag, name_w = name_w);
            }
            println!();
            println!("Use 'vscode-profile-importer extensions --profile <name>' to list extensions.");
            println!("Use 'vscode-profile-importer uninstall --profile <name>' to remove extensions.");
        }
        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
    }
}

// ── cmd_extensions ─────────────────────────────────────────────────────────

fn cmd_extensions(args: ExtensionsArgs) {
    let storage = resolve_storage(args.storage_json);
    check_storage(&storage);
    let code = require_code_cli();

    // Validate profile exists
    let profiles = imp::list_profiles(&storage).unwrap_or_default();
    if !profiles.iter().any(|p| p.name == args.profile) {
        eprintln!(
            "Error: Profile '{}' not found.\nRun 'vscode-profile-importer list' to see available profiles.",
            args.profile
        );
        std::process::exit(1);
    }

    match imp::list_extensions_for_profile(&args.profile, &code) {
        Ok(exts) => {
            println!("Extensions in profile '{}' ({}):", args.profile, exts.len());
            println!("{}", "─".repeat(72));
            if exts.is_empty() {
                println!("  (none)");
            } else {
                for (i, ext) in exts.iter().enumerate() {
                    println!("  {:3}.  {}", i + 1, ext);
                }
            }
            println!("{}", "─".repeat(72));
        }
        Err(e) => { eprintln!("Error: {}", e); std::process::exit(1); }
    }
}

// ── cmd_remove ─────────────────────────────────────────────────────────────

fn cmd_remove(args: RemoveArgs) {
    let storage = resolve_storage(args.storage_json);
    check_storage(&storage);
    let code = require_code_cli();

    // Validate profile exists
    let profiles = imp::list_profiles(&storage).unwrap_or_default();
    if !profiles.iter().any(|p| p.name == args.profile) {
        eprintln!(
            "Error: Profile '{}' not found.\nRun 'vscode-profile-importer list' to see available profiles.",
            args.profile
        );
        std::process::exit(1);
    }

    // Verify the extension is installed; give a helpful hint on near-miss
    let installed = imp::list_extensions_for_profile(&args.profile, &code)
        .unwrap_or_default();

    let exact = installed.iter().find(|e| e.to_lowercase() == args.extension.to_lowercase());
    if exact.is_none() {
        let hints: Vec<&String> = installed.iter()
            .filter(|e| e.to_lowercase().contains(&args.extension.to_lowercase()))
            .collect();
        if hints.is_empty() {
            eprintln!("Error: Extension '{}' is not installed in profile '{}'.", args.extension, args.profile);
        } else {
            eprintln!("Error: Extension '{}' not found in profile '{}'.", args.extension, args.profile);
            eprintln!("Did you mean one of:");
            for h in hints { eprintln!("  {}", h); }
        }
        std::process::exit(1);
    }
    let ext_id = exact.unwrap().clone();

    println!("Profile   : {}", args.profile);
    println!("Extension : {}", ext_id);
    println!();

    if !args.force && !args.dry_run {
        if !confirm("Uninstall this extension?") {
            println!("Aborted.");
            return;
        }
    }

    if args.dry_run {
        println!("[dry-run] would uninstall: {}", ext_id);
        return;
    }

    print!("Uninstalling...");
    let _ = io::stdout().flush();
    match imp::uninstall_extension(&args.profile, &ext_id, &code, false) {
        Ok(()) => println!(" done"),
        Err(e) => { println!(" FAILED"); eprintln!("Error: {}", e); std::process::exit(1); }
    }
    println!("Removed '{}' from profile '{}'.", ext_id, args.profile);
}

// ── cmd_uninstall ──────────────────────────────────────────────────────────

fn cmd_uninstall(args: UninstallArgs) {
    let storage = resolve_storage(args.storage_json);
    check_storage(&storage);
    let code = require_code_cli();

    // Validate profile exists
    let profiles = imp::list_profiles(&storage).unwrap_or_default();
    if !profiles.iter().any(|p| p.name == args.profile) {
        eprintln!(
            "Error: Profile '{}' not found.\nRun 'vscode-profile-importer list' to see available profiles.",
            args.profile
        );
        std::process::exit(1);
    }

    // Gather extensions
    eprint!("Fetching extensions for profile '{}'...", args.profile);
    let _ = io::stderr().flush();
    let exts = imp::list_extensions_for_profile(&args.profile, &code).unwrap_or_default();
    eprintln!(" {} found", exts.len());

    if exts.is_empty() {
        println!("No extensions installed in profile '{}'. Nothing to do.", args.profile);
        return;
    }

    println!();
    println!("Profile    : {}", args.profile);
    println!("Extensions : {}", exts.len());
    if args.dry_run { println!("Mode       : DRY RUN (no changes will be made)"); }
    println!();

    let mut removed = 0usize;
    let mut skipped = 0usize;
    let mut failed  = 0usize;

    if args.force {
        // Bulk mode — one confirmation prompt then remove all
        println!("WARNING: This will uninstall ALL {} extensions from profile '{}'.", exts.len(), args.profile);
        println!();
        for e in &exts { println!("  - {}", e); }
        println!();

        if !args.dry_run && !confirm(&format!("Confirm bulk removal of all {} extensions?", exts.len())) {
            println!("Aborted. No extensions were removed.");
            return;
        }
        println!();

        for ext in &exts {
            if args.dry_run {
                println!("  [dry-run] would uninstall: {}", ext);
                removed += 1;
            } else {
                print!("  Uninstalling {}...", ext);
                let _ = io::stdout().flush();
                match imp::uninstall_extension(&args.profile, ext, &code, false) {
                    Ok(())  => { println!(" done");   removed += 1; }
                    Err(_)  => { println!(" FAILED"); failed  += 1; }
                }
            }
        }
    } else {
        // Interactive mode — prompt per extension
        println!("You will be prompted for each extension. Press Ctrl+C to abort.");
        println!();

        for (i, ext) in exts.iter().enumerate() {
            println!("[{}/{}] {}", i + 1, exts.len(), ext);

            if args.dry_run {
                println!("  [dry-run] would uninstall.");
                removed += 1;
                println!();
                continue;
            }

            if confirm("  Uninstall this extension?") {
                print!("  Uninstalling...");
                let _ = io::stdout().flush();
                match imp::uninstall_extension(&args.profile, ext, &code, false) {
                    Ok(())  => { println!(" done\n");   removed += 1; }
                    Err(_)  => { println!(" FAILED\n"); failed  += 1; }
                }
            } else {
                println!("  Skipped.\n");
                skipped += 1;
            }
        }
    }

    println!("{}", "─".repeat(72));
    if args.dry_run {
        println!("Dry run complete. {} extension(s) would be removed.", removed);
    } else {
        print!("Done.  Removed: {}  ", removed);
        if !args.force { print!("Skipped: {}  ", skipped); }
        println!("Failed: {}", failed);
    }
}

// ── main ───────────────────────────────────────────────────────────────────

fn main() {
    let cli = Cli::parse();
    match cli.command {
        SubCmd::Import(a)     => cmd_import(a),
        SubCmd::List(a)       => cmd_list(a),
        SubCmd::Extensions(a) => cmd_extensions(a),
        SubCmd::Remove(a)     => cmd_remove(a),
        SubCmd::Uninstall(a)  => cmd_uninstall(a),
    }
}
