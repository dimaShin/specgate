# Manual release and distribution

This workflow publishes versioned artifacts from a local machine without CI/CD.

## Targets

- macOS arm64 (`aarch64-apple-darwin`)
- Linux x86_64 glibc (`x86_64-unknown-linux-gnu`)

## 1) Build versioned artifacts

From repository root:

```bash
./scripts/release-local.sh
```

Optional explicit version override:

```bash
./scripts/release-local.sh 0.1.0
```

Artifacts are written to:

- `dist/v<VERSION>/specgate-v<VERSION>-aarch64-apple-darwin.tar.gz` (when built)
- `dist/v<VERSION>/specgate-v<VERSION>-x86_64-unknown-linux-gnu.tar.gz` (when built)
- `dist/v<VERSION>/SHA256SUMS`

The Linux artifact is produced on Linux hosts directly, or on macOS via Docker (`linux/amd64`) when Docker is available.

## 2) Verify checksums

```bash
cd dist/v<VERSION>
shasum -a 256 -c SHA256SUMS
```

## 3) Publish artifacts

Upload both tarballs and `SHA256SUMS` to a versioned release location (for example a GitHub release page).

## 4) Sidecar usage in local applications

Example sidecar launch:

```bash
SPECGATE_REGISTRY_DIR=/tmp/specgate-localtest/registry \
./target/release/specgate runtime serve \
  --config ./runtime.yaml \
  --mode proxy \
  --upstream http://127.0.0.1:8080 \
  --listen 127.0.0.1:18080
```

Point your application to `http://127.0.0.1:18080` for sidecar testing.

## 5) Homebrew tap publishing (manual)

Generate a formula from local release checksums:

```bash
./scripts/generate-homebrew-formula.sh --version <VERSION> --repo <owner>/<repo>
```

Default output path is `dist/v<VERSION>/specgate.rb`. You can override with `--output`.

Create/update a formula in your tap repository using the generated file and your published release URLs.

Example formula skeleton:

```ruby
class Specgate < Formula
  desc "specgate CLI runtime"
  homepage "https://github.com/<owner>/<repo>"
  version "0.1.0"

  on_macos do
    on_arm do
      url "https://github.com/<owner>/<repo>/releases/download/v0.1.0/specgate-v0.1.0-aarch64-apple-darwin.tar.gz"
      sha256 "<SHA256_FROM_SHA256SUMS>"
    end
  end

  def install
    bin.install "specgate"
  end

  test do
    system "#{bin}/specgate", "version"
  end
end
```

Release update flow for Homebrew:

1. Publish new artifact tarball and `SHA256SUMS`.
2. Update formula `version`, `url`, and `sha256`.
3. Commit formula change in tap repo.
4. Validate with `brew install --build-from-source <tap>/specgate` or `brew reinstall <tap>/specgate`.
