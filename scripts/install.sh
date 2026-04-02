#!/usr/bin/env bash

#
# git_agent_race_controller - CLI Installer (garc)
#

set -euo pipefail

PACKAGE_NAME="garc"
BIN_NAME="garc"
REPO_URL="https://github.com/0xBoji/git_agent_race_controller"

BOLD="$(tput bold 2>/dev/null || echo '')"
GREY="$(tput setaf 8 2>/dev/null || echo '')"
BLUE="$(tput setaf 4 2>/dev/null || echo '')"
MAGENTA="$(tput setaf 5 2>/dev/null || echo '')"
CYAN="$(tput setaf 6 2>/dev/null || echo '')"
GREEN="$(tput setaf 2 2>/dev/null || echo '')"
YELLOW="$(tput setaf 3 2>/dev/null || echo '')"
RED="$(tput setaf 1 2>/dev/null || echo '')"
RESET="$(tput sgr0 2>/dev/null || echo '')"

info() { echo -e "${CYAN}${BOLD}info:${RESET} $1"; }
warn() { echo -e "${YELLOW}${BOLD}warn:${RESET} $1" >&2; }
error() { echo -e "${RED}${BOLD}error:${RESET} $1" >&2; }
success() { echo -e "${GREEN}${BOLD}success:${RESET} $1"; }

banner() {
  cat <<EOF

${MAGENTA}${BOLD} ██████╗  █████╗ ██████╗  ██████╗
██╔════╝ ██╔══██╗██╔══██╗██╔════╝
██║  ███╗███████║██████╔╝██║     
██║   ██║██╔══██║██╔══██╗██║     
╚██████╔╝██║  ██║██║  ██║╚██████╗
 ╚═════╝ ╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝
${RESET}${BLUE}╔══════════════════════════════════════════════════════╗
║ git agent race controller • safe branch coordination ║
╚══════════════════════════════════════════════════════╝${RESET}

${BOLD}git_agent_race_controller Installer${RESET}
${GREY}CAMP-aware Git branch collision guard${RESET}

EOF
}

usage() {
  cat <<EOF
Usage:
  install.sh [options]

Options:
  --git    Install directly from GitHub
  --force  Reinstall even if the binary is already present
  -h, --help  Show this help message

EOF
}

require_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    error "required command not found: $1"
    if [[ "$1" == "cargo" ]]; then
      info "Rust/Cargo is required to build ${BIN_NAME}."
      info "Install it from https://rustup.rs/ and try again."
    fi
    exit 1
  fi
}

SOURCE="crates"
FORCE=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    --git)
      SOURCE="git"
      shift
      ;;
    --force)
      FORCE=1
      shift
      ;;
    -h|--help)
      banner
      usage
      exit 0
      ;;
    *)
      error "unknown argument: $1"
      usage >&2
      exit 1
      ;;
  esac
done

banner
require_cmd cargo

INSTALL_ARGS=(install --locked --bin "$BIN_NAME")
if [[ "$FORCE" -eq 1 ]]; then
  INSTALL_ARGS+=(--force)
fi

if [[ "$SOURCE" == "git" ]]; then
  info "Installing ${BOLD}${BIN_NAME}${RESET} from GitHub (${REPO_URL})..."
  if cargo "${INSTALL_ARGS[@]}" --git "$REPO_URL"; then
    echo
    success "Successfully installed ${BOLD}${BIN_NAME}${RESET} from source."
  else
    error "Failed to install from GitHub."
    exit 1
  fi
else
  info "Installing ${BOLD}${BIN_NAME}${RESET} from crates.io (${PACKAGE_NAME})..."
  if cargo "${INSTALL_ARGS[@]}" "$PACKAGE_NAME" 2>/dev/null; then
    echo
    success "Successfully installed ${BOLD}${BIN_NAME}${RESET} from crates.io."
  else
    warn "Failed to install from crates.io."
    info "The package might not be published yet. Trying GitHub fallback..."
    echo
    info "Running: cargo ${INSTALL_ARGS[*]} --git ${REPO_URL}"
    if cargo "${INSTALL_ARGS[@]}" --git "$REPO_URL"; then
      echo
      success "Successfully installed ${BOLD}${BIN_NAME}${RESET} from GitHub fallback."
    else
      error "Failed to install from both crates.io and GitHub fallback."
      echo
      info "Check your network connection or try manually:"
      info "  cargo install --git ${REPO_URL} --bin ${BIN_NAME}"
      exit 1
    fi
  fi
fi

echo
info "Try it out:"
echo -e "  ${BOLD}${BIN_NAME} --help${RESET}"
echo
