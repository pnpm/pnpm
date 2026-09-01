#!/usr/bin/env bash

# Build the pnpm RPM for RED OS 8 (and any other RHEL 8 derivative) from a
# released standalone Linux x64 tarball, plus a dnf repository that installs on
# a host with no network access at all.
#
#   packaging/rpm/build.sh <version> [pnpm-linux-x64.tar.gz]
#
# Run this on RHEL 8 or a derivative — see packaging/rpm/README.md for the
# container invocation. The runtime dependencies bundled into the repository are
# the ones missing from the system that builds it, so building elsewhere yields
# a repository that cannot install offline.
#
# Outputs into $OUT_DIR (default dist/rpm):
#   pnpm-<version>-1.x86_64.rpm
#   repo/                                    dnf repository (repodata + pnpm.repo)
#   pnpm-<version>-redos8-x86_64-repo.tar.gz the same repository, packed
#
# Requires: rpmbuild, createrepo_c, dnf, binutils, curl, tar.

set -euo pipefail

version=${1:?usage: build.sh <version> [pnpm-linux-x64.tar.gz]}
version=${version#v}
tarball=${2:-}
out_dir=${OUT_DIR:-$PWD/dist/rpm}
spec_dir=$(cd "$(dirname "$0")" && pwd)

# RPM version fields cannot contain '-'. '~' sorts before the release it
# precedes, so 11.1.0~rc.1 upgrades to 11.1.0.
rpm_version=${version//-/'~'}

# RED OS 8 provides glibc 2.28, the RHEL 8 baseline. A binary referencing a
# newer symbol version installs fine and then dies at exec.
readonly max_glibc=2.28

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

mkdir -p "$work/SOURCES" "$work/payload"
if [ -n "$tarball" ]; then
  cp "$tarball" "$work/SOURCES/pnpm-linux-x64.tar.gz"
else
  curl -fsSL --retry 3 --retry-delay 2 -o "$work/SOURCES/pnpm-linux-x64.tar.gz" \
    "https://github.com/pnpm/pnpm/releases/download/v${version}/pnpm-linux-x64.tar.gz"
fi
cp "$spec_dir/pnpm.sh" "$work/SOURCES/"

tar -xzf "$work/SOURCES/pnpm-linux-x64.tar.gz" -C "$work/payload"

required_glibc=$(
  {
    objdump -T "$work/payload/pnpm"
    find "$work/payload/dist" -name '*.node' -exec objdump -T {} +
  } | grep -o 'GLIBC_[0-9][0-9.]*' | sed 's/^GLIBC_//' | sort -uV | tail -1
)
if [ "$(printf '%s\n%s\n' "$required_glibc" "$max_glibc" | sort -V | tail -1)" != "$max_glibc" ]; then
  echo "pnpm $version needs glibc $required_glibc, RED OS 8 provides $max_glibc" >&2
  exit 1
fi

rpmbuild -bb "$spec_dir/pnpm.spec" \
  --define "_topdir $work" \
  --define "pnpm_version $rpm_version" \
  --target x86_64

rpm_file=$(find "$work/RPMS/x86_64" -name '*.rpm' -print -quit)
test -n "$rpm_file"

# Named rather than globbed: OUT_DIR may be a directory that holds more than
# this script's output, and a stale RPM left beside the current one would be
# picked up by anything copying the directory.
mkdir -p "$out_dir"
rm -rf "${out_dir:?}/repo"
rm -f "$out_dir"/pnpm-*.rpm "$out_dir"/pnpm-*-redos8-x86_64-repo.tar.gz
mkdir -p "$out_dir/repo"

# The offline target resolves the package's Requires against its own installed
# packages and nothing else, so whatever this system is missing has to travel in
# the repository. Downloading through the package itself is what makes dnf name
# them; `--resolve` on the dependency names would drag in every architecture's
# build of each.
dnf install -y --downloadonly --destdir="$work/deps" "$rpm_file"
find "$work/deps" -name '*.rpm' ! -name 'pnpm-*' -exec cp {} "$out_dir/repo/" \;

cp "$rpm_file" "$out_dir/"
cp "$rpm_file" "$out_dir/repo/"
cat > "$out_dir/repo/pnpm.repo" <<'EOF'
[pnpm]
name=pnpm
baseurl=file:///opt/pnpm-repo
enabled=1
gpgcheck=0
EOF
createrepo_c --quiet "$out_dir/repo"

tar czf "$out_dir/pnpm-${version}-redos8-x86_64-repo.tar.gz" -C "$out_dir" repo

echo "Built $(basename "$rpm_file") for glibc $required_glibc in $out_dir"
