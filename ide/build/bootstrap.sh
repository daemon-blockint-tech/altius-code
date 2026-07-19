#!/usr/bin/env bash
# Bootstraps a full "Altius IDE" build on top of a pristine microsoft/vscode
# checkout: clones the pinned upstream tag, deep-merges ide/product.json over
# it, and bundles the altius-agent extension as a built-in.
#
# This does the actual fork checkout + merge; it does NOT run the VS Code
# build itself (that's `yarn && yarn compile` / `./scripts/code.sh`, see
# ide/README.md) — those steps need a full Node/native-modules toolchain and
# a large download that don't belong in an unattended script.
set -euo pipefail

IDE_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VSCODE_DIR="${IDE_DIR}/vscode"
VSCODE_REF="${ALTIUS_VSCODE_REF:-1.94.2}" # pinned upstream tag; bump deliberately

if [ -d "${VSCODE_DIR}/.git" ]; then
  echo "==> ${VSCODE_DIR} already exists, skipping clone (delete it to re-clone)"
else
  echo "==> Cloning microsoft/vscode @ ${VSCODE_REF} into ${VSCODE_DIR}"
  git clone --depth 1 --branch "${VSCODE_REF}" \
    https://github.com/microsoft/vscode.git "${VSCODE_DIR}"
fi

echo "==> Merging ide/product.json over vscode/product.json"
node "${IDE_DIR}/build/merge-product-json.js" \
  "${VSCODE_DIR}/product.json" \
  "${IDE_DIR}/product.json" \
  "${VSCODE_DIR}/product.json"

echo "==> Bundling altius-agent as a built-in extension"
mkdir -p "${VSCODE_DIR}/extensions/altius-agent"
rsync -a --delete \
  --exclude node_modules \
  --exclude out \
  --exclude '.vsix' \
  "${IDE_DIR}/extensions/altius-agent/" \
  "${VSCODE_DIR}/extensions/altius-agent/"

cat <<'EOF'

==> Bootstrap complete.

Next steps (inside ide/vscode), see ide/README.md for details:
  1. corepack enable && yarn install
  2. yarn compile
  3. ./scripts/code.sh   (Linux/macOS) or .\scripts\code.bat (Windows)

The bundled altius-agent extension still needs its own compile step first:
  (cd ide/extensions/altius-agent && npm install && npm run compile)
EOF
