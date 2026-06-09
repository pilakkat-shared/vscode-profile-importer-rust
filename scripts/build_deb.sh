#!/usr/bin/env bash
set -euo pipefail

#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CRATE_DIR="$ROOT"
OUTDIR="$CRATE_DIR/target/debian/manual"
DEBOUT="$CRATE_DIR/target/debian/vscode-profile-importer_0.1.0-1_amd64.deb"

rm -rf "$OUTDIR"
mkdir -p "$OUTDIR/DEBIAN"
mkdir -p "$OUTDIR/usr/bin"
mkdir -p "$OUTDIR/usr/share/vscode-profile-importer/examples"
mkdir -p "$OUTDIR/usr/share/doc/vscode-profile-importer/examples"
mkdir -p "$OUTDIR/usr/share/man/man1"

# Build release
(cd "$CRATE_DIR" && cargo build --release)

# Copy binary
cp "$CRATE_DIR/target/release/vscode-profile-importer" "$OUTDIR/usr/bin/vscode-profile-importer"
chmod 755 "$OUTDIR/usr/bin/vscode-profile-importer"

# Copy packaging_files (self-contained assets) into the package
if [ -d "$CRATE_DIR/packaging_files" ]; then
  cp -a "$CRATE_DIR/packaging_files/usr/share/vscode-profile-importer/examples/"* "$OUTDIR/usr/share/vscode-profile-importer/examples/" || true
  cp -a "$CRATE_DIR/packaging_files/usr/share/doc/vscode-profile-importer/"* "$OUTDIR/usr/share/doc/vscode-profile-importer/" || true
  cp -a "$CRATE_DIR/packaging_files/usr/share/man/man1/"* "$OUTDIR/usr/share/man/man1/" || true
  cp -a "$CRATE_DIR/packaging_files/usr/bin/"* "$OUTDIR/usr/bin/" || true
fi

# Simple control file
cat > "$OUTDIR/DEBIAN/control" <<EOF
Package: vscode-profile-importer
Version: 0.1.0-1
Section: utils
Priority: optional
Architecture: amd64
Maintainer: vscode-profile-importer <noreply@example.com>
Depends: bash
Description: Safe, fault-tolerant VS Code profile importer (Rust port)
EOF

dpkg-deb --build "$OUTDIR" "$DEBOUT"
echo "Deb created at $DEBOUT"
