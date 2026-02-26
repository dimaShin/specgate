#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="${1:-}"

if [[ -z "$VERSION" ]]; then
  VERSION="$(awk -F '"' '/^version = "[0-9]+\.[0-9]+\.[0-9]+"/{print $2; exit}' "$ROOT_DIR/Cargo.toml")"
fi

if [[ -z "$VERSION" ]]; then
  echo "Failed to resolve version from Cargo.toml" >&2
  exit 1
fi

DIST_DIR="$ROOT_DIR/dist/v$VERSION"
STAGE_DIR="$ROOT_DIR/target/release-stage"
DOCKER_IMAGE="${SPECGATE_RELEASE_DOCKER_IMAGE:-rust:1-bookworm}"
BINARY_COUNT=0
rm -rf "$DIST_DIR" "$STAGE_DIR"
mkdir -p "$DIST_DIR" "$STAGE_DIR"

build_macos_arm64() {
  cargo build --release --locked --target aarch64-apple-darwin --target-dir "$ROOT_DIR/target-macos-arm64"
  cp "$ROOT_DIR/target-macos-arm64/aarch64-apple-darwin/release/specgate" "$DIST_DIR/specgate-v$VERSION-aarch64-apple-darwin"
  BINARY_COUNT=$((BINARY_COUNT + 1))
}

build_linux_x86_64() {
  if [[ "$(uname -s)" == "Linux" && "$(uname -m)" == "x86_64" ]]; then
    cargo build --release --locked --target-dir "$ROOT_DIR/target-linux-x86_64"
    cp "$ROOT_DIR/target-linux-x86_64/release/specgate" "$DIST_DIR/specgate-v$VERSION-x86_64-unknown-linux-gnu"
    BINARY_COUNT=$((BINARY_COUNT + 1))
    return
  fi

  if command -v docker >/dev/null 2>&1; then
    if docker run --rm --platform linux/amd64 \
      -v "$ROOT_DIR:/work" \
      -w /work \
      "$DOCKER_IMAGE" \
      bash -lc "if ! command -v cargo >/dev/null 2>&1; then export PATH=\"/usr/local/cargo/bin:\$HOME/.cargo/bin:\$PATH\"; fi; cargo build --release --locked --target-dir /work/target-linux-x86_64"; then
      cp "$ROOT_DIR/target-linux-x86_64/release/specgate" "$DIST_DIR/specgate-v$VERSION-x86_64-unknown-linux-gnu"
      BINARY_COUNT=$((BINARY_COUNT + 1))
    else
      echo "Skipping Linux artifact: Docker is installed but unavailable" >&2
    fi
    return
  fi

  echo "Skipping Linux artifact: Docker unavailable on non-Linux host" >&2
}

if [[ "$(uname -s)" == "Darwin" && "$(uname -m)" == "arm64" ]]; then
  build_macos_arm64
else
  echo "Skipping macOS arm64 artifact: host is not Darwin arm64" >&2
fi

build_linux_x86_64

if [[ "$BINARY_COUNT" -eq 0 ]]; then
  echo "No artifacts were built" >&2
  exit 1
fi

for binary_path in "$DIST_DIR"/specgate-v$VERSION-*; do
  [[ -f "$binary_path" ]] || continue
  archive_name="$(basename "$binary_path").tar.gz"
  bundle_dir="$STAGE_DIR/${archive_name%.tar.gz}"
  mkdir -p "$bundle_dir"
  cp "$binary_path" "$bundle_dir/specgate"
  chmod +x "$bundle_dir/specgate"
  cp "$ROOT_DIR/README.md" "$bundle_dir/README.md"
  cp "$ROOT_DIR/LICENSE" "$bundle_dir/LICENSE"
  cp "$ROOT_DIR/LICENSE-ADDENDUM.md" "$bundle_dir/LICENSE-ADDENDUM.md"
  tar -C "$bundle_dir" -czf "$DIST_DIR/$archive_name" .
done

(
  cd "$DIST_DIR"
  shasum -a 256 specgate-v$VERSION-*.tar.gz > SHA256SUMS
)

echo "Release artifacts generated in $DIST_DIR"
ls -1 "$DIST_DIR"
