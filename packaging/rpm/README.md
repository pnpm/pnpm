# pnpm RPM for RED OS 8

An RPM of the standalone pnpm build — the same binary
`https://get.pnpm.io/install.sh` installs, bundled with its own Node.js runtime
— for RED OS 8, RHEL 8 derivatives, and newer RPM-based systems on `x86_64`.
The package targets the RHEL 8 baseline (glibc 2.28, rpm 4.14). RED OS 8 ships
glibc 2.36, above that baseline.

The package installs:

| Path                       | Contents                                        |
| -------------------------- | ----------------------------------------------- |
| `/usr/lib/pnpm/`           | the standalone binary and its `dist/` payload   |
| `/usr/bin/pnpm`            | symlink to it                                   |
| `/etc/profile.d/pnpm.sh`   | `PNPM_HOME` and `PATH`, as the install script sets them |

## Where to get it

Every pnpm release page carries two RPM assets, built by the release workflow
from that release's `pnpm-linux-x64.tar.gz`:

- `pnpm-<version>-1.el8.x86_64.rpm`, the bare package.
- `pnpm-<version>-redos8-x86_64-repo.tar.gz`, a dnf repository with `repodata`,
  a ready `pnpm.repo`, and the runtime dependencies a stock RHEL 8 install is
  missing (`libatomic`).

Both carry a build provenance attestation:

```sh
gh attestation verify pnpm-11.25.0-redos8-x86_64-repo.tar.gz --repo pnpm/pnpm
```

## Install on a host without network access

Copy the repository tarball into the image and unpack it at the path its
`pnpm.repo` points to:

```sh
tar xzf pnpm-11.25.0-redos8-x86_64-repo.tar.gz -C /tmp
rm -rf /opt/pnpm-repo
mv /tmp/repo /opt/pnpm-repo
cp /opt/pnpm-repo/pnpm.repo /etc/yum.repos.d/
dnf install -y --disablerepo='*' --enablerepo=pnpm pnpm
```

The bare `.rpm` installs directly too, if the host already has `libatomic` and
a repository is more than you need:

```sh
dnf install -y ./pnpm-11.25.0-1.el8.x86_64.rpm
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
