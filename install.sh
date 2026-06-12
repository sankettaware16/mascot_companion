#!/usr/bin/env bash
# Build Companion in release mode and install it for the current user (Linux).
# No root required. Re-run any time to update.
set -euo pipefail

cd "$(dirname "$0")"

BIN_DIR="${HOME}/.local/bin"
APP_DIR="${HOME}/.local/share/applications"
CFG_DIR="${HOME}/.config/companion"

echo "==> Building (release)…"
cargo build --release

echo "==> Installing binaries -> ${BIN_DIR}/"
mkdir -p "${BIN_DIR}"
install -m 0755 target/release/companion "${BIN_DIR}/companion"
install -m 0755 target/release/companion-convert "${BIN_DIR}/companion-convert"

echo "==> Seeding default config -> ${CFG_DIR}/config.toml (kept if it exists)"
mkdir -p "${CFG_DIR}"
[ -f "${CFG_DIR}/config.toml" ] || cp config.toml "${CFG_DIR}/config.toml"

echo "==> Creating launcher entry"
mkdir -p "${APP_DIR}"
cat > "${APP_DIR}/companion.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=Companion
Comment=A tiny living pixel desktop pet
Exec=${BIN_DIR}/companion
Terminal=false
Categories=Utility;
EOF

echo
echo "Done! Launch with:  companion"
echo "(Companion reads ./config.toml if present, else ${CFG_DIR}/config.toml.)"
case ":${PATH}:" in
  *":${BIN_DIR}:"*) ;;
  *) echo "Note: ${BIN_DIR} isn't on your PATH — add it to use the 'companion' command." ;;
esac
