#!/usr/bin/env bash
set -Eeuo pipefail

BINARY_URL="https://github.com/mrhappynice/bplus-searchrs/releases/download/v0.4.1.2/bplus-searchrs"
JSON_URL="https://github.com/mrhappynice/bplus-searchrs/raw/refs/heads/main/.env"
INSTALL_DIR="bplus-searchrs"
BINARY_NAME="bplus-searchrs"
JSON_NAME=".env"
INDEX_NAME="search-apis.md"

INDEX_URL="https://github.com/mrhappynice/bplus-searchrs/raw/refs/heads/main/search-apis.md"




# --- helpers ---
have() { command -v "$1" >/dev/null 2>&1; }

download() {
  # $1 = url, $2 = output file
  if have curl; then
    curl -fL --proto '=https' --tlsv1.2 --retry 3 --retry-delay 1 --progress-bar -o "$2" "$1"
  elif have wget; then
    wget --https-only --tries=3 -O "$2" "$1"
  else
    echo "Error: need 'curl' or 'wget' to download files." >&2
    exit 1
  fi
}

# --- work ---
echo "Creating '${INSTALL_DIR}' (if needed)…"
mkdir -p "${INSTALL_DIR}"
cd "${INSTALL_DIR}"

echo "Downloading binary -> ${BINARY_NAME}"
download "${BINARY_URL}" "${BINARY_NAME}"

echo "Downloading json -> ${JSON_NAME}"
download "${JSON_URL}" "${JSON_NAME}"

echo "Making '${BINARY_NAME}' executable…"
chmod +x "${BINARY_NAME}"

echo "Getting extra files.."
download "${INDEX_URL}" "${INDEX_NAME}" 


echo "Done ✅"
echo
echo "Files installed to: $(pwd)"
echo " - ${BINARY_NAME}"
echo " - ${JSON_NAME}"
echo
echo "Run it with:"
echo "  ./$(printf %q "${BINARY_NAME}")"
