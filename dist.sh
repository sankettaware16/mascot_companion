#!/usr/bin/env bash
# Build shareable bundles for Linux and Windows from this Linux machine.
#   ./dist.sh        ->  dist/companion-vX.Y.Z-linux-x86_64.tar.gz
#                        dist/companion-vX.Y.Z-windows-x86_64.zip
#
# Windows cross-build needs mingw:  sudo apt install gcc-mingw-w64-x86-64
# macOS can't be cross-built from Linux (Apple SDK licensing) — use the
# GitHub Actions workflow in .github/workflows/release.yml instead.
set -euo pipefail
cd "$(dirname "$0")"

VERSION=$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
OUT=dist
rm -rf "$OUT"
mkdir -p "$OUT"

echo "==> Linux x86_64"
cargo build --release
STAGE="$OUT/companion"
mkdir -p "$STAGE"
cp target/release/companion target/release/companion-convert \
   config.toml README.md install.sh "$STAGE/"
tar -C "$OUT" -czf "$OUT/companion-v$VERSION-linux-x86_64.tar.gz" companion
rm -rf "$STAGE"

echo "==> Windows x86_64 (mingw cross-compile)"
rustup target add x86_64-pc-windows-gnu >/dev/null 2>&1 || true
cargo build --release --target x86_64-pc-windows-gnu
STAGE="$OUT/companion"
mkdir -p "$STAGE"
cp target/x86_64-pc-windows-gnu/release/companion.exe \
   target/x86_64-pc-windows-gnu/release/companion-convert.exe \
   config.toml README.md "$STAGE/"
cat > "$STAGE/START-HERE.txt" <<'EOF'
COMPANION — desktop pet
=======================
1. Double-click companion.exe  (no install needed)
2. Edit config.toml to customise; restart to apply.
Note: on Windows the pet roams on its own — cursor-following, grab/throw,
window-climbing and notification-chasing are Linux-only for now.
EOF
(cd "$OUT" && zip -qr "companion-v$VERSION-windows-x86_64.zip" companion)
rm -rf "$STAGE"

echo
ls -la "$OUT"/*.tar.gz "$OUT"/*.zip
echo
echo "Done. Share those two files directly — no installer needed."
echo "For macOS bundles: push to GitHub and tag a release (see README)."
