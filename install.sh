#!/bin/sh
set -eu

repo="pstwh/dirstat"
bin_dir="${DIRSTAT_BIN_DIR:-$HOME/.local/bin}"

fail() {
    echo "dirstat: $*" >&2
    exit 1
}

need() {
    command -v "$1" > /dev/null 2>&1 || fail "$1 is required"
}

need curl
need uname

os=$(uname -s)
arch=$(uname -m)
case "$os-$arch" in
    Linux-x86_64 | Linux-amd64) target="x86_64-linux" ext="tar.gz" ;;
    Darwin-arm64 | Darwin-aarch64) target="aarch64-macos" ext="zip" ;;
    *) fail "no build for $os $arch, see https://github.com/$repo#build" ;;
esac

if [ -n "${DIRSTAT_VERSION:-}" ]; then
    tag="v${DIRSTAT_VERSION#v}"
else
    latest=$(curl -fsSLI -o /dev/null -w '%{url_effective}' "https://github.com/$repo/releases/latest")
    tag=${latest##*/}
    case "$tag" in
        v*) ;;
        *) fail "could not find the latest release" ;;
    esac
fi

name="dirstat-$tag-$target"
base="https://github.com/$repo/releases/download/$tag"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

echo "Downloading dirstat $tag for $target"
curl -fL# -o "$work/$name.$ext" "$base/$name.$ext"
curl -fsSL -o "$work/SHA256SUMS.txt" "$base/SHA256SUMS.txt"

expected=$(grep " $name.$ext\$" "$work/SHA256SUMS.txt" | cut -d' ' -f1)
[ -n "$expected" ] || fail "$name.$ext is missing from SHA256SUMS.txt"
if command -v sha256sum > /dev/null 2>&1; then
    actual=$(sha256sum "$work/$name.$ext" | cut -d' ' -f1)
else
    actual=$(shasum -a 256 "$work/$name.$ext" | cut -d' ' -f1)
fi
[ "$expected" = "$actual" ] || fail "checksum mismatch for $name.$ext"

mkdir -p "$bin_dir"

if [ "$target" = "x86_64-linux" ]; then
    need tar
    tar -xzf "$work/$name.$ext" -C "$work"
    install -m 755 "$work/$name/dirstat" "$bin_dir/dirstat"
    apps="${XDG_DATA_HOME:-$HOME/.local/share}/applications"
    icons="${XDG_DATA_HOME:-$HOME/.local/share}/icons/hicolor/256x256/apps"
    mkdir -p "$apps" "$icons"
    sed "s|^Exec=dirstat|Exec=$bin_dir/dirstat|" "$work/$name/packaging/dirstat.desktop" > "$apps/dirstat.desktop"
    install -m 644 "$work/$name/packaging/dirstat.png" "$icons/dirstat.png"
    command -v update-desktop-database > /dev/null 2>&1 && update-desktop-database "$apps" > /dev/null 2>&1 || true
    echo "Installed $bin_dir/dirstat and a menu entry"
else
    need unzip
    unzip -q "$work/$name.$ext" -d "$work"
    apps="${DIRSTAT_APP_DIR:-$HOME/Applications}"
    mkdir -p "$apps"
    rm -rf "$apps/dirstat.app"
    mv "$work/$name/dirstat.app" "$apps/dirstat.app"
    xattr -dr com.apple.quarantine "$apps/dirstat.app" 2> /dev/null || true
    ln -sf "$apps/dirstat.app/Contents/MacOS/dirstat" "$bin_dir/dirstat"
    echo "Installed $apps/dirstat.app and $bin_dir/dirstat"
fi

case ":$PATH:" in
    *":$bin_dir:"*) ;;
    *) echo "Add $bin_dir to your PATH to run dirstat from a terminal" ;;
esac
