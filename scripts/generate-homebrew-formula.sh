#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST_ROOT="$ROOT_DIR/dist"
VERSION=""
BASE_URL=""
REPO=""
HOMEPAGE=""
OUTPUT=""

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      VERSION="$2"
      shift 2
      ;;
    --base-url)
      BASE_URL="$2"
      shift 2
      ;;
    --repo)
      REPO="$2"
      shift 2
      ;;
    --homepage)
      HOMEPAGE="$2"
      shift 2
      ;;
    --output)
      OUTPUT="$2"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

if [[ -z "$VERSION" ]]; then
  VERSION="$(find "$DIST_ROOT" -maxdepth 1 -type d -name 'v*' -print 2>/dev/null | sed 's|.*/v||' | sort -V | tail -n 1)"
  if [[ -z "$VERSION" ]]; then
    echo "No dist versions found under $DIST_ROOT" >&2
    exit 1
  fi
fi

DIST_DIR="$DIST_ROOT/v$VERSION"
CHECKSUM_FILE="$DIST_DIR/SHA256SUMS"

if [[ ! -f "$CHECKSUM_FILE" ]]; then
  echo "Missing checksum file: $CHECKSUM_FILE" >&2
  exit 1
fi

if [[ -z "$BASE_URL" && -n "$REPO" ]]; then
  BASE_URL="https://github.com/$REPO/releases/download/v$VERSION"
fi

if [[ -z "$BASE_URL" ]]; then
  echo "Provide --base-url or --repo to build release URLs" >&2
  exit 1
fi

if [[ -z "$HOMEPAGE" ]]; then
  if [[ -n "$REPO" ]]; then
    HOMEPAGE="https://github.com/$REPO"
  else
    HOMEPAGE="$BASE_URL"
  fi
fi

macos_archive="specgate-v$VERSION-aarch64-apple-darwin.tar.gz"
linux_archive="specgate-v$VERSION-x86_64-unknown-linux-gnu.tar.gz"

macos_sha="$(awk -v file="$macos_archive" '$2==file {print $1}' "$CHECKSUM_FILE")"
linux_sha="$(awk -v file="$linux_archive" '$2==file {print $1}' "$CHECKSUM_FILE")"

if [[ -z "$macos_sha" ]]; then
  echo "Missing macOS checksum entry for $macos_archive" >&2
  exit 1
fi

if [[ -z "$OUTPUT" ]]; then
  OUTPUT="$DIST_DIR/specgate.rb"
fi

mkdir -p "$(dirname "$OUTPUT")"

{
  echo "class Specgate < Formula"
  echo "  desc \"specgate CLI runtime\""
  echo "  homepage \"$HOMEPAGE\""
  echo "  version \"$VERSION\""
  echo ""
  echo "  on_macos do"
  echo "    on_arm do"
  echo "      url \"$BASE_URL/$macos_archive\""
  echo "      sha256 \"$macos_sha\""
  echo "    end"
  echo "  end"
  if [[ -n "$linux_sha" ]]; then
    echo ""
    echo "  on_linux do"
    echo "    on_intel do"
    echo "      url \"$BASE_URL/$linux_archive\""
    echo "      sha256 \"$linux_sha\""
    echo "    end"
    echo "  end"
  fi
  echo ""
  echo "  def install"
  echo "    bin.install \"specgate\""
  echo "  end"
  echo ""
  echo "  test do"
  echo "    system \"#{bin}/specgate\", \"version\""
  echo "  end"
  echo "end"
} > "$OUTPUT"

echo "Generated Homebrew formula: $OUTPUT"
