#!/bin/sh
# Install grove: https://github.com/AgentsORG/grove
#
#   curl -fsSL https://raw.githubusercontent.com/AgentsORG/grove/main/install.sh | sh
#
# Works on macOS, Linux and Windows (Git Bash, MSYS2, Cygwin). Settings:
#   GROVE_VERSION       a release tag like v0.1.0 (default: the latest)
#   GROVE_INSTALL_DIR   where the binary goes (default: ~/.grove/bin)
set -eu

repo="${GROVE_REPO:-AgentsORG/grove}"
version="${GROVE_VERSION:-latest}"
install_dir="${GROVE_INSTALL_DIR:-${HOME:-}/.grove/bin}"

fail() {
  printf 'grove install: %s\n' "$1" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

[ -n "$install_dir" ] || fail 'HOME is not set; set GROVE_INSTALL_DIR to choose where to install'
need uname
need mkdir
need mktemp
need rm

os="$(uname -s)"
arch="$(uname -m)"
exe=""
ext="tar.gz"
case "$os" in
  Darwin) os_part="apple-darwin" ;;
  Linux) os_part="unknown-linux-musl" ;;
  MINGW* | MSYS* | CYGWIN* | Windows_NT)
    os_part="pc-windows-msvc"
    exe=".exe"
    ext="zip"
    ;;
  *) fail "unsupported operating system: $os (build from source: cargo install --git https://github.com/$repo grove-cli)" ;;
esac
case "$arch" in
  x86_64 | amd64) cpu="x86_64" ;;
  arm64 | aarch64) cpu="aarch64" ;;
  *) fail "unsupported CPU: $arch (build from source: cargo install --git https://github.com/$repo grove-cli)" ;;
esac
target="$cpu-$os_part"
asset="grove-$target.$ext"

if [ -n "${GROVE_DOWNLOAD_BASE:-}" ]; then
  base="$GROVE_DOWNLOAD_BASE"
elif [ "$version" = "latest" ]; then
  base="https://github.com/$repo/releases/latest/download"
else
  base="https://github.com/$repo/releases/download/$version"
fi

download() {
  if command -v curl >/dev/null 2>&1; then
    curl -fsSL "$1" -o "$2"
  elif command -v wget >/dev/null 2>&1; then
    wget -q "$1" -O "$2"
  else
    fail 'need curl or wget'
  fi
}

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT INT TERM

printf 'Downloading grove for %s (%s)\n' "$target" "$version"
download "$base/$asset" "$tmp/$asset" || fail "could not download $base/$asset"

# Verify the checksum when the release has one and a tool is available.
if download "$base/$asset.sha256" "$tmp/$asset.sha256" 2>/dev/null; then
  expected="$(awk '{print $1}' "$tmp/$asset.sha256")"
  if command -v sha256sum >/dev/null 2>&1; then
    actual="$(sha256sum "$tmp/$asset" | awk '{print $1}')"
  elif command -v shasum >/dev/null 2>&1; then
    actual="$(shasum -a 256 "$tmp/$asset" | awk '{print $1}')"
  else
    actual="$expected"
  fi
  [ "$expected" = "$actual" ] || fail "checksum mismatch for $asset"
fi

case "$ext" in
  tar.gz) need tar && tar -xzf "$tmp/$asset" -C "$tmp" ;;
  zip)
    if command -v unzip >/dev/null 2>&1; then
      unzip -q "$tmp/$asset" -d "$tmp"
    else
      powershell.exe -NoProfile -Command "Expand-Archive -Force '$tmp/$asset' '$tmp'" || fail 'need unzip to extract'
    fi
    ;;
esac

bin="$(find "$tmp" -type f -name "grove$exe" | head -n 1)"
[ -n "$bin" ] || fail "the download did not contain grove$exe"
mkdir -p "$install_dir"
mv -f "$bin" "$install_dir/grove$exe"
chmod +x "$install_dir/grove$exe" 2>/dev/null || true

printf 'Installed %s\n' "$("$install_dir/grove$exe" --version)"
case ":$PATH:" in
  *":$install_dir:"*) ;;
  *)
    printf '\nAdd grove to your PATH:\n  export PATH="$PATH:%s"\n' "$install_dir"
    printf 'Put that line in your shell profile (~/.zshrc, ~/.bashrc) to keep it.\n'
    ;;
esac
printf '\nFor agents: npx skills add AgentsORG/grove\n'
