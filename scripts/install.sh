#!/bin/sh
# Installs Dioramic from the GitHub releases on macOS and Linux.
#
#   curl -fsSL https://github.com/JonathanSantos/dioramic/releases/latest/download/install.sh | sh
#   curl -fsSL https://github.com/JonathanSantos/dioramic/releases/latest/download/install.sh | sh -s -- --desktop
#
# Options (each also has an environment variable):
#   --desktop          install the desktop app instead of the CLI          (DIORAMIC_DESKTOP=1)
#   --version <tag>    a release tag such as v0.1.0; default: latest       (DIORAMIC_VERSION)
#   --dir <path>       where the CLI binary goes; default: ~/.local/bin    (DIORAMIC_INSTALL_DIR)
#   --dry-run          print what would be downloaded and exit
#
# Asset names must stay in sync with .github/workflows/release.yml.
set -eu

REPO="${DIORAMIC_REPO:-JonathanSantos/dioramic}"
VERSION="${DIORAMIC_VERSION:-latest}"
INSTALL_DIR="${DIORAMIC_INSTALL_DIR:-$HOME/.local/bin}"
DESKTOP="${DIORAMIC_DESKTOP:-0}"
DRY_RUN=0

say() { printf 'dioramic: %s\n' "$*"; }
fail() { printf 'dioramic: error: %s\n' "$*" >&2; exit 1; }

parse_args() {
    while [ $# -gt 0 ]; do
        case "$1" in
            --desktop) DESKTOP=1 ;;
            --version) [ $# -ge 2 ] || fail "--version needs a tag"; VERSION="$2"; shift ;;
            --dir) [ $# -ge 2 ] || fail "--dir needs a path"; INSTALL_DIR="$2"; shift ;;
            --dry-run) DRY_RUN=1 ;;
            -h | --help) sed -n '2,13p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//'; exit 0 ;;
            *) fail "unknown option: $1" ;;
        esac
        shift
    done
}

release_url() {
    if [ "$VERSION" = "latest" ]; then
        printf 'https://github.com/%s/releases/latest/download' "$REPO"
    else
        case "$VERSION" in v*) tag="$VERSION" ;; *) tag="v$VERSION" ;; esac
        printf 'https://github.com/%s/releases/download/%s' "$REPO" "$tag"
    fi
}

detect_os() {
    case "$(uname -s)" in
        Darwin) echo macos ;;
        Linux) echo linux ;;
        *) fail "unsupported OS $(uname -s); on Windows use install.ps1" ;;
    esac
}

detect_arch() {
    case "$(uname -m)" in
        x86_64 | amd64) arch=x86_64 ;;
        arm64 | aarch64) arch=aarch64 ;;
        *) fail "unsupported CPU architecture $(uname -m)" ;;
    esac
    # A shell running under Rosetta reports x86_64 on Apple Silicon; prefer the native binary.
    if [ "$arch" = x86_64 ] && [ "$(uname -s)" = Darwin ] \
        && [ "$(sysctl -n sysctl.proc_translated 2>/dev/null || echo 0)" = 1 ]; then
        arch=aarch64
    fi
    echo "$arch"
}

cli_target() {
    case "$1" in
        macos) echo "$2-apple-darwin" ;;
        linux) echo "$2-unknown-linux-gnu" ;;
    esac
}

download() {
    if command -v curl >/dev/null 2>&1; then
        curl --proto '=https' --tlsv1.2 -fsSL "$1" -o "$2"
    elif command -v wget >/dev/null 2>&1; then
        wget -q "$1" -O "$2"
    else
        fail "curl or wget is required"
    fi
}

# Checks the file against the `<asset>.sha256` published next to it; skipped when no hashing tool exists.
verify_checksum() {
    file="$1"
    download "$2.sha256" "$file.sha256"
    expected=$(cut -d ' ' -f 1 <"$file.sha256")
    if command -v sha256sum >/dev/null 2>&1; then
        actual=$(sha256sum "$file" | cut -d ' ' -f 1)
    elif command -v shasum >/dev/null 2>&1; then
        actual=$(shasum -a 256 "$file" | cut -d ' ' -f 1)
    else
        say "no sha256sum/shasum found; skipping checksum verification"
        return 0
    fi
    [ "$expected" = "$actual" ] || fail "checksum mismatch for $(basename "$file")"
}

fetch() {
    say "downloading $1"
    download "$1" "$2"
    verify_checksum "$2" "$1"
}

install_cli() {
    target=$(cli_target "$1" "$2")
    asset="dioramic-cli-$target.tar.gz"
    url="$(release_url)/$asset"
    if [ "$DRY_RUN" = 1 ]; then
        say "would download $url"
        say "would install $INSTALL_DIR/dioramic"
        return 0
    fi
    fetch "$url" "$WORK_DIR/$asset"
    tar -xzf "$WORK_DIR/$asset" -C "$WORK_DIR"
    mkdir -p "$INSTALL_DIR"
    install -m 755 "$WORK_DIR/dioramic-cli-$target/dioramic" "$INSTALL_DIR/dioramic"
    say "installed $INSTALL_DIR/dioramic ($("$INSTALL_DIR/dioramic" --version 2>/dev/null || echo 'version unknown'))"
    case ":$PATH:" in
        *":$INSTALL_DIR:"*) ;;
        *) say "add $INSTALL_DIR to your PATH, e.g.: echo 'export PATH=\"$INSTALL_DIR:\$PATH\"' >> ~/.profile" ;;
    esac
    say "next: run 'dioramic init' in your project to register the MCP server with your AI clients"
}

install_desktop_macos() {
    asset="dioramic-desktop-macos-universal.dmg"
    url="$(release_url)/$asset"
    apps_dir=/Applications
    [ -w "$apps_dir" ] || apps_dir="$HOME/Applications"
    if [ "$DRY_RUN" = 1 ]; then
        say "would download $url"
        say "would install $apps_dir/Dioramic.app"
        return 0
    fi
    fetch "$url" "$WORK_DIR/$asset"
    mount_point="$WORK_DIR/mount"
    mkdir -p "$mount_point" "$apps_dir"
    hdiutil attach "$WORK_DIR/$asset" -nobrowse -quiet -mountpoint "$mount_point"
    rm -rf "$apps_dir/Dioramic.app"
    cp -R "$mount_point/Dioramic.app" "$apps_dir/"
    hdiutil detach "$mount_point" -quiet
    say "installed $apps_dir/Dioramic.app"
}

install_desktop_linux() {
    asset="dioramic-desktop-linux-$1.AppImage"
    url="$(release_url)/$asset"
    if [ "$DRY_RUN" = 1 ]; then
        say "would download $url"
        say "would install $INSTALL_DIR/dioramic-desktop"
        return 0
    fi
    fetch "$url" "$WORK_DIR/$asset"
    mkdir -p "$INSTALL_DIR"
    install -m 755 "$WORK_DIR/$asset" "$INSTALL_DIR/dioramic-desktop"
    say "installed $INSTALL_DIR/dioramic-desktop (AppImage; .deb and .rpm packages are on the release page)"
}

main() {
    parse_args "$@"
    os=$(detect_os)
    arch=$(detect_arch)
    WORK_DIR=$(mktemp -d)
    trap 'rm -rf "$WORK_DIR"' EXIT INT TERM
    if [ "$DESKTOP" = 1 ]; then
        case "$os" in
            macos) install_desktop_macos ;;
            linux) install_desktop_linux "$arch" ;;
        esac
    else
        install_cli "$os" "$arch"
    fi
}

main "$@"
