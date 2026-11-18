# pnpm RPM for RHEL 8 and newer

An RPM of the standalone pnpm build — the same binary
`https://get.pnpm.io/install.sh` installs, bundled with its own Node.js runtime
— for `x86_64` RHEL 8 and newer, and the distributions built on it, such as
AlmaLinux, Rocky Linux, and Oracle Linux. The package targets the RHEL 8
baseline: glibc 2.28 and rpm 4.14.

The package installs:

| Path                       | Contents                                        |
| -------------------------- | ----------------------------------------------- |
| `/usr/lib/pnpm/`           | the standalone binary, its `pn`, `pnpx`, and `pnx` aliases, and its `dist/` payload |
| `/usr/bin/pnpm`, `pn`, `pnpx`, `pnx` | symlinks to them                      |
| `/etc/profile.d/pnpm.sh`   | `PNPM_HOME` and `PATH`, as the install script sets them |

## Where to get it

pnpm releases made after the release workflow gained its RPM job carry two RPM
assets, built from that release's `pnpm-linux-x64.tar.gz`. For an earlier
release, [build them locally](#build-locally).

- `pnpm-<version>-1.el8.x86_64.rpm`, the bare package.
- `pnpm-<version>-el8-x86_64-repo.tar.gz`, a dnf repository with `repodata`,
  a ready `pnpm.repo`, and the runtime dependencies a stock RHEL 8 install is
  missing (`libatomic`).

Both carry a build provenance attestation:

```sh
gh attestation verify pnpm-<version>-el8-x86_64-repo.tar.gz --repo pnpm/pnpm
```

## Install on a host without network access

Copy the repository tarball into the image and unpack it at the path its
`pnpm.repo` points to:

```sh
tar xzf pnpm-<version>-el8-x86_64-repo.tar.gz -C /tmp
rm -rf /opt/pnpm-repo
mv /tmp/repo /opt/pnpm-repo
cp /opt/pnpm-repo/pnpm.repo /etc/yum.repos.d/
dnf install -y --disablerepo='*' --enablerepo=pnpm pnpm
```

The bare `.rpm` installs directly too, if the host already has `libatomic` and
a repository is more than you need:

```sh
dnf install -y ./pnpm-<version>-1.el8.x86_64.rpm
```

Node.js is not a dependency and is not bundled as a runtime you can call: pnpm
carries its own copy for itself. Install one for your project with
`pnpm runtime set node 22 -g` or let `devEngines.runtime` in `package.json` do
it.

The repository is unsigned (`gpgcheck=0`), so whoever ships it into an image is
responsible for the integrity of the copy. Verify its attestation as shown
above, or sign the RPM with your own key and set `gpgcheck=1` before
distributing it further.

## Build locally

Build inside RHEL 8, not on the host: the repository bundles the runtime
dependencies missing from the system that builds it, so building on a
different distribution produces a repository that cannot install offline.

```sh
docker run --rm -v "$PWD:/src" -w /src almalinux:8 bash -euc '
  dnf install -y rpm-build createrepo_c dnf-plugins-core binutils tar curl
  packaging/rpm/build.sh 11.25.0
'
```

Pass a local `pnpm-linux-x64.tar.gz` as a second argument to package a build
that is not on GitHub releases. The build fails if the binary needs a glibc
newer than 2.28.

`build.sh` packages only releases that ship `pnpm-linux-x64.tar.gz`, which is
pnpm 11.0.0 and newer. Releases before the RPM assets were added can be
packaged this way.
