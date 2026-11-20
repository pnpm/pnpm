#!/bin/sh
# Installs the pnpm release tarball into /opt/pnpm and links it onto PATH.
#
# Environment:
#   PNPM_VERSION       pnpm version to install, without a leading "v"
#   PNPM_ASSET_SUFFIX  release asset suffix after the architecture: "" or "-musl"
#   PNPM_SHA256_AMD64  sha256 of the amd64 tarball
#   PNPM_SHA256_ARM64  sha256 of the arm64 tarball
#   TARGETARCH         set by BuildKit
set -eu

test -n "$PNPM_VERSION"
case "$TARGETARCH" in
  amd64) arch=x64; expected_sha="$PNPM_SHA256_AMD64" ;;
  arm64) arch=arm64; expected_sha="$PNPM_SHA256_ARM64" ;;
  *) echo "unsupported architecture: $TARGETARCH" >&2; exit 1 ;;
esac
test -n "$expected_sha" || { echo "missing PNPM_SHA256_* build-arg for $TARGETARCH" >&2; exit 1; }

asset="pnpm-linux-${arch}${PNPM_ASSET_SUFFIX}.tar.gz"
mkdir -p /opt/pnpm "$PNPM_HOME/bin"
chmod -R a+rwX "$PNPM_HOME"
curl -fsSL --retry 3 --retry-delay 2 -o /tmp/pnpm.tgz \
  "https://github.com/pnpm/pnpm/releases/download/v${PNPM_VERSION}/${asset}"
actual_sha="$(sha256sum /tmp/pnpm.tgz | awk '{print $1}')"
test "$actual_sha" = "$expected_sha" || {
  echo "sha256 mismatch for ${asset}: expected $expected_sha, got $actual_sha" >&2
  exit 1
}
tar -xzf /tmp/pnpm.tgz -C /opt/pnpm
rm /tmp/pnpm.tgz
ln -s /opt/pnpm/pnpm /usr/local/bin/pnpm
for alias in pn pnpx pnx; do
  ln /opt/pnpm/pnpm "/opt/pnpm/$alias"
  ln -s "/opt/pnpm/$alias" "/usr/local/bin/$alias"
done
for bin in pnpm pn; do
  installed="$("$bin" --version)"
  test "$installed" = "$PNPM_VERSION" || {
    echo "$bin version mismatch: expected $PNPM_VERSION, got $installed" >&2
    exit 1
  }
done
for alias in pnpx pnx; do
  help="$("$alias" --help)"
  case "$help" in
    *"pnpm dlx"*) ;;
    *) echo "$alias does not run pnpm dlx" >&2; exit 1 ;;
  esac
done
