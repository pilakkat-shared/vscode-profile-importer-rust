Rust reimplementation and packaging helper

Build
  make build

Release and package
  make release
  make dist

Create .deb (requires cargo-deb)
  ./scripts/build_deb.sh

The deb metadata is configured in Cargo.toml under [package.metadata.deb].

Profiles written by this importer
  When importing, the tool writes VS Code-compatible profile directories using
  the layout: <profiles-dir>/<safe-basename>.code-profile/profile.json
  where <safe-basename> is produced by make_safe_basename(name). This ensures
  the profile is discovered by the VS Code CLI (code --list-profiles).

Safe basename rules
  - Keeps ASCII alphanumerics, dot (.), dash (-) and underscore (_)
  - Replaces whitespace and other characters with '-'
  - Collapses repeated '-' and trims leading/trailing '-'
  - Limits length to 60 characters, falls back to 'profile' if empty

Packaged examples
  Example profiles are installed to /usr/share/vscode-profile-importer/examples
  in the package. See package_files/usr/share/doc/vscode-profile-importer/README_EXAMPLES.md
  for details.
