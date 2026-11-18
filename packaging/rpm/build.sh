#!/usr/bin/env bash

# Build the pnpm RPM for RHEL 8 and newer from a released standalone Linux x64
# tarball, plus a dnf repository that installs on a host with no network access
# at all.
#
#   packaging/rpm/build.sh <version> [pnpm-linux-x64.tar.gz]
#
# Run this on RHEL 8 or a derivative — see packaging/rpm/README.md for the
# container invocation. The runtime dependencies bundled into the repository are
# the ones missing from the system that builds it, so building elsewhere yields
# a repository that cannot install offline.
#
# Outputs into $OUT_DIR (default dist/rpm):
#   pnpm-<version>-1.el8.x86_64.rpm
#   repo/                                    dnf repository (repodata + pnpm.repo)
#   pnpm-<version>-el8-x86_64-repo.tar.gz the same repository, packed
#
# Requires: rpmbuild, createrepo_c, dnf, dnf-plugins-core, binutils, curl, tar.

set -euo pipefail

version=${1:?usage: build.sh <version> [pnpm-linux-x64.tar.gz]}
version=${version#v}
tarball=${2:-}
out_dir=${OUT_DIR:-$PWD/dist/rpm}
spec_dir=$(cd "$(dirname "$0")" && pwd)

# RPM version fields cannot contain '-'. '~' sorts before the release it
# precedes, so 11.1.0~rc.1 upgrades to 11.1.0.
rpm_version=${version//-/'~'}

# The package targets the RHEL 8 baseline, glibc 2.28. A binary that needs a
# newer glibc installs fine and then dies at exec.
readonly max_glibc=2.28

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

mkdir -p "$work/SOURCES" "$work/payload"
if [ -n "$tarball" ]; then
  cp "$tarball" "$work/SOURCES/pnpm-linux-x64.tar.gz"
else
  curl -fsSL --retry 3 --retry-delay 2 --connect-timeout 30 --max-time 300 --speed-time 30 --speed-limit 1000 \
    -o "$work/SOURCES/pnpm-linux-x64.tar.gz" \
    "https://github.com/pnpm/pnpm/releases/download/v${version}/pnpm-linux-x64.tar.gz"
fi
cp "$spec_dir/pnpm.sh" "$work/SOURCES/"

tar -xzf "$work/SOURCES/pnpm-linux-x64.tar.gz" -C "$work/payload"

# The ELF version references, not the symbol table: a binary can also require
# a non-numeric tag such as GLIBC_ABI_DT_RELR, which glibc 2.28 lacks.
version_info=$(objdump -p "$work/payload/pnpm")
version_info+=$'\n'$(find "$work/payload/dist" -name '*.node' -exec objdump -p {} +)
glibc_refs=$(grep -oE 'GLIBC_[A-Za-z0-9_.]+' <<<"$version_info" | sort -u)
unsupported_tags=$(grep -v '^GLIBC_[0-9][0-9.]*$' <<<"$glibc_refs" || true)
if [ -n "$unsupported_tags" ]; then
  echo "pnpm $version requires glibc tags missing from glibc $max_glibc:" $unsupported_tags >&2
  exit 1
fi
required_glibc=$(sed 's/^GLIBC_//' <<<"$glibc_refs" | sort -V | tail -1)
if [ "$(printf '%s\n%s\n' "$required_glibc" "$max_glibc" | sort -V | tail -1)" != "$max_glibc" ]; then
  echo "pnpm $version needs glibc $required_glibc, above the RHEL 8 baseline of $max_glibc" >&2
  exit 1
fi

rpmbuild -bb "$spec_dir/pnpm.spec" \
  --define "_topdir $work" \
  --define "pnpm_version $rpm_version" \
  --target x86_64

rpm_file=$(find "$work/RPMS/x86_64" -name '*.rpm' -print -quit)
test -n "$rpm_file"
# The file carries the release version even when the package version has '~',
# so a prerelease's asset names match its tag. dnf reads the version from the
# package header, not the file name.
rpm_name=$(basename "$rpm_file")
rpm_name=${rpm_name/"$rpm_version"/"$version"}

# Outputs are assembled in a staging directory inside OUT_DIR, so replacing the
# previous ones is a rename on one filesystem, done only once all are built.
mkdir -p "$out_dir"
stage=$(mktemp -d "$out_dir/.stage.XXXXXX")
trap 'rm -rf "$work" "$stage"' EXIT
mkdir -p "$stage/repo"

# The offline target resolves the package's Requires against its own installed
# packages and nothing else, so whatever this system is missing has to travel in
# the repository. Downloading through the package itself is what makes dnf name
# them; `--resolve` on the dependency names would drag in every architecture's
# build of each.
dnf install -y --downloadonly --destdir="$work/deps" "$rpm_file"
find "$work/deps" -name '*.rpm' ! -name 'pnpm-*' -exec cp -t "$stage/repo/" -- {} +
if [ -z "$(find "$stage/repo" -name 'libatomic*.rpm' -print -quit)" ]; then
  dnf download --destdir="$stage/repo" --arch x86_64 libatomic
fi
test -n "$(find "$stage/repo" -name 'libatomic*.rpm' -print -quit)"

cp "$rpm_file" "$stage/$rpm_name"
cp "$rpm_file" "$stage/repo/$rpm_name"
cat > "$stage/repo/pnpm.repo" <<'EOF'
[pnpm]
name=pnpm
baseurl=file:///opt/pnpm-repo
enabled=1
gpgcheck=0
EOF
createrepo_c --quiet "$stage/repo"

tar czf "$stage/pnpm-${version}-el8-x86_64-repo.tar.gz" -C "$stage" repo

# Only this script's own outputs are removed: OUT_DIR may hold other files, and
# a stale RPM left beside the current one would be picked up by anything copying
# the directory.
rm -rf "${out_dir:?}/repo"
rm -f "$out_dir"/pnpm-*.rpm "$out_dir"/pnpm-*-el8-x86_64-repo.tar.gz
mv "$stage"/* "$out_dir/"

echo "Built $rpm_name for glibc $required_glibc in $out_dir"
