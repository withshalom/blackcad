#!/usr/bin/env bash
# Usage: package-release.sh ARTIFACTS_DIR OUT_DIR
#
# Puts the files a build run made under the names the README links to, in
# OUT_DIR, with a SHA256SUMS.txt over them. ARTIFACTS_DIR holds the run's
# artifacts, each in a folder of its own name (what download-artifact makes):
#   cad-linux-x86_64/cad  cad-macos-arm64/cad  cad-windows-x86_64/cad.exe
#   cad-viewer-debug-apk/app-debug.apk
#   cad-viewer-release-apk/app-release.apk (only when the run could sign)
#   blackcad-web/ (the web app: index.html, core.wasm, ...)
#   single-file/BlackCAD.html
# Fails, naming the file, when any of them is missing. The names are the
# contract with README.md ("Download") and .github/release-notes.md.
set -euo pipefail
in=$1
mkdir -p "$2"
out=$(cd "$2" && pwd)

# The one file called $2 under the artifact folder $1, or a failure.
pick() {
  local f
  f=$(find "$in/$1" -type f -name "$2" 2>/dev/null | head -n 1 || true)
  if [ -z "$f" ]; then
    # To stderr: stdout is what the callers capture or discard.
    echo "::error::the artifact $1 has no $2" >&2
    exit 1
  fi
  printf '%s\n' "$f"
}

stage=$(mktemp -d)
trap 'rm -rf "$stage"' EXIT

# The command-line build. Artifacts do not keep the executable bit, so it is
# set again here, and the .tar.gz carries it to wherever it is unpacked.
for target in linux-x86_64 macos-arm64; do
  bin=$(pick "cad-$target" cad)
  mkdir "$stage/$target"
  install -m 0755 "$bin" "$stage/$target/cad"
  tar -czf "$out/blackcad-cli-$target.tar.gz" -C "$stage/$target" cad
done
exe=$(pick cad-windows-x86_64 cad.exe)
(cd "$(dirname "$exe")" && zip -q "$out/blackcad-cli-windows-x86_64.zip" cad.exe)

apk=$(pick cad-viewer-debug-apk app-debug.apk)
cp "$apk" "$out/blackcad-viewer-android-debug.apk"
# The signed release APK, when the run had the signing secrets. It is the
# one to install; the debug APK is always offered as well.
if [ -d "$in/cad-viewer-release-apk" ]; then
  cp "$(pick cad-viewer-release-apk app-release.apk)" "$out/blackcad-viewer-android.apk"
fi
page=$(pick single-file BlackCAD.html)
cp "$page" "$out/BlackCAD.html"

# The web app, the files the pages job checks before it publishes them.
for f in index.html core.wasm sw.js manifest.webmanifest; do
  pick blackcad-web "$f" >/dev/null
done
(cd "$in/blackcad-web" && zip -q -r "$out/blackcad-web.zip" .)

cd "$out"
files=(*)
sha256sum -- "${files[@]}" >SHA256SUMS.txt
ls -l
cat SHA256SUMS.txt
