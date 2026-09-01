# pnpm RPM for RED OS 8

An RPM of the standalone pnpm build — the same binary
`https://get.pnpm.io/install.sh` installs, bundled with its own Node.js runtime
— for RED OS 8 and other RHEL 8 derivatives (`x86_64`, glibc 2.28, rpm 4.14).

The package installs:

| Path                       | Contents                                        |
| -------------------------- | ----------------------------------------------- |
| `/usr/lib/pnpm/`           | the standalone binary and its `dist/` payload   |
| `/usr/bin/pnpm`            | symlink to it                                   |
| `/etc/profile.d/pnpm.sh`   | `PNPM_HOME` and `PATH`, as the install script sets them |

## Where to get it

[`.github/workflows/rpm.yml`](../../.github/workflows/rpm.yml) builds it on every
published release, and on demand via `workflow_dispatch` with a version. Download
the `pnpm-rpm-<version>-x86_64` artifact from the run — GitHub releases in this
repository are immutable once published, so the artifacts cannot be attached to
them afterwards.

The artifact holds the bare `.rpm` and `pnpm-<version>-redos8-x86_64-repo.tar.gz`,
a dnf repository with `repodata` and a ready `pnpm.repo`.

## Install on a host without network access

Copy the repository tarball into the image and unpack it at the path its
`pnpm.repo` points to:

```sh
tar xzf pnpm-11.0.0-redos8-x86_64-repo.tar.gz -C /tmp
mv /tmp/repo /opt/pnpm-repo
cp /opt/pnpm-repo/pnpm.repo /etc/yum.repos.d/
dnf install -y pnpm
```

The bare `.rpm` installs directly too, if a repository is more than you need:

```sh
dnf install -y ./pnpm-11.0.0-1.x86_64.rpm
```

Node.js is not a dependency and is not bundled as a runtime you can call: pnpm
carries its own copy for itself. Install one for your project with
`pnpm runtime set node 22 -g` or let `devEngines.runtime` in `package.json` do it.

## Build locally

```sh
packaging/rpm/build.sh 11.0.0
```

Needs `rpmbuild`, `createrepo_c`, `binutils`, `curl` and `tar`. Pass a local
`pnpm-linux-x64.tar.gz` as a second argument to package a build that is not on
GitHub releases. The build fails if the binary needs a glibc newer than the 2.28
that RED OS 8 provides.
