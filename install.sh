#!/usr/bin/env bash
# install.sh — installer for trelloc (Unofficial Trello CLI + MCP server)
#
# Usage:
#   curl -fsSL https://raw.githubusercontent.com/mulhamna/trello-commands/main/install.sh | bash
#
# Options (env vars):
#   TRELLOC_VERSION=v1.0.0      install a specific release (default: latest)
#   TRELLOC_INSTALL_DIR=/path   install directory (default: ~/.local/bin)
#
# Downloads the release archive for your OS/arch, verifies it against the
# release's checksums.txt, and installs the `trelloc` binary.
# Not affiliated with or endorsed by Trello.

set -euo pipefail

REPO="mulhamna/trello-commands"
BINARY="trelloc"

if [ -t 1 ]; then
  RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BOLD='\033[1m'; RESET='\033[0m'
else
  RED=''; GREEN=''; YELLOW=''; BOLD=''; RESET=''
fi

info()  { printf "${GREEN}==>${RESET} ${BOLD}%s${RESET}\n" "$*"; }
warn()  { printf "${YELLOW}warning:${RESET} %s\n" "$*"; }
error() { printf "${RED}error:${RESET} %s\n" "$*" >&2; exit 1; }

detect_platform() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os" in
    Linux)
      case "$arch" in
        x86_64|amd64)  echo "${BINARY}-linux-x86_64" ;;
        aarch64|arm64) echo "${BINARY}-linux-aarch64" ;;
        *) error "Unsupported Linux architecture: $arch" ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        x86_64) echo "${BINARY}-macos-x86_64" ;;
        arm64)  echo "${BINARY}-macos-aarch64" ;;
        *) error "Unsupported macOS architecture: $arch" ;;
      esac
      ;;
    *)
      error "Unsupported OS: $os. On Windows use install.ps1 or scoop."
      ;;
  esac
}

sha256() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{print $1}'
  else
    error "Need sha256sum or shasum to verify the download."
  fi
}

latest_tag() {
  local tag
  tag="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\([^"]*\)".*/\1/')"
  [ -n "$tag" ] || error "Could not determine the latest release from the GitHub API."
  echo "$tag"
}

install_dir() {
  local dir="${TRELLOC_INSTALL_DIR:-$HOME/.local/bin}"
  mkdir -p "$dir" 2>/dev/null || error "Cannot create $dir; set TRELLOC_INSTALL_DIR to a writable directory."
  [ -w "$dir" ] || error "$dir is not writable; set TRELLOC_INSTALL_DIR."
  echo "$dir"
}

main() {
  for cmd in curl tar; do
    command -v "$cmd" >/dev/null 2>&1 || error "Missing required tool: $cmd"
  done

  local platform tag dir
  platform="$(detect_platform)"
  tag="${TRELLOC_VERSION:-$(latest_tag)}"
  case "$tag" in v*) ;; *) tag="v$tag" ;; esac
  dir="$(install_dir)"

  info "Installing ${BINARY} ${tag} (${platform}) to ${dir}"

  local base="https://github.com/${REPO}/releases/download/${tag}"
  local archive="${platform}.tar.gz"
  TMP_DIR="$(mktemp -d)"
  trap 'rm -rf "$TMP_DIR"' EXIT
  local tmp="$TMP_DIR"

  curl -fsSL "${base}/${archive}" -o "${tmp}/${archive}" || error "Download failed: ${base}/${archive}"
  curl -fsSL "${base}/checksums.txt" -o "${tmp}/checksums.txt" || error "Download failed: ${base}/checksums.txt"

  local expected actual
  expected="$(awk -v f="$archive" '$2 == f || $2 ~ ("/" f "$") { print $1 }' "${tmp}/checksums.txt" | head -1)"
  [ -n "$expected" ] || error "No checksum for ${archive} in checksums.txt"
  actual="$(sha256 "${tmp}/${archive}")"
  [ "$expected" = "$actual" ] || error "Checksum mismatch for ${archive}: expected ${expected}, got ${actual}"
  info "Checksum verified"

  tar -xzf "${tmp}/${archive}" -C "$tmp"
  [ -f "${tmp}/${platform}" ] || error "Archive did not contain ${platform}"
  install -m 0755 "${tmp}/${platform}" "${dir}/${BINARY}"

  printf "\n${GREEN}%s %s installed to %s${RESET}\n" "$BINARY" "$tag" "${dir}/${BINARY}"

  case ":$PATH:" in
    *":$dir:"*) ;;
    *)
      echo ""
      warn "${dir} is not in your PATH. Add this to your shell profile:"
      echo "    export PATH=\"${dir}:\$PATH\""
      ;;
  esac

  echo ""
  echo "  Next: trelloc auth login"
  echo "  MCP:  trelloc mcp install --client claude-code"
  echo "  Docs: https://github.com/${REPO}"
}

main "$@"
