Example Profiles
----------------

This file describes the example profiles packaged with vscode-profile-importer
and the extensions each contains. The examples are installed at
/usr/share/vscode-profile-importer/examples when the package is installed.

1) example.code-profile
   - Purpose: Minimal placeholder profile used by the packaged systemd service
   - Settings: none
   - Extensions: none

2) webdev.code-profile
   - Purpose: Web development starter profile
   - Settings:
     - editor.formatOnSave: true
   - Extensions:
     - esbenp.prettier-vscode — Prettier (code formatter)
     - ritwickdey.liveserver — Live Server
     - eamodio.gitlens — GitLens (Git supercharged)
     - dbaeumer.vscode-eslint — ESLint integration

3) rust.code-profile
   - Purpose: Rust development starter profile
   - Settings:
     - rust-analyzer.cargo.runBuildScripts: true
   - Extensions:
     - rust-lang.rust-analyzer — rust-analyzer (language server)
     - vadimcn.vscode-lldb — CodeLLDB (debugger)
     - swellaby.rust-mod — Rust mod helper

4) python.code-profile
   - Purpose: Python development starter profile
   - Settings:
     - python.formatting.provider: black
   - Extensions:
     - ms-python.python — Microsoft Python extension
     - ms-python.vscode-pylance — Pylance language server
     - ms-toolsai.jupyter — Jupyter support

Using the examples
------------------

To test a profile without installing extensions, run the importer in dry-run mode:

  vscode-profile-importer --dry-run /usr/share/vscode-profile-importer/examples/webdev.code-profile

To enable the packaged example timer (user-level) and use the example profile,
run the helper and then inspect the timer status:

  install_systemd_user --yes
  systemctl --user status vscode-profile-importer-sync.timer

Notes about profile layout
  The importer writes VS Code-compatible profile directories at the destination:
  <profiles-dir>/<safe-basename>.code-profile/profile.json. The safe basename is
  generated from the profile's internal "name" field using the rules described
  in the project README. This ensures the profile is discoverable by the
  VS Code CLI (code --list-profiles).
