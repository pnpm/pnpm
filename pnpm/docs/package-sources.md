---
id: package-sources
title: Supported package sources
---

pnpm supports installing packages from various sources. These sources are divided into two categories: **trusted sources** and **exotic sources**.

Exotic sources (like Git repositories or direct tarball URLs) can introduce supply chain risks when used by transitive dependencies. You can prevent transitive dependencies from using exotic sources by setting [`blockExoticSubdeps`] to `true`.

[`blockExoticSubdeps`]: ./settings/dependency-resolution.md#blockexoticsubdeps

## Trusted sources

Trusted sources are considered safe for both direct and transitive dependencies.

### npm registry

`pnpm add package-name` will install the latest version of `package-name` from
the [npm registry](https://www.npmjs.com/) by default.

If executed in a workspace, the command will first try to check whether other
projects in the workspace use the specified package. If so, the already used version range
will be installed.

You may also install packages by:

* tag: `pnpm add express@nightly`
* version: `pnpm add express@1.0.0`
* version range: `pnpm add express@2 react@">=0.1.0 <0.2.0"`

### JSR registry

Added in: v10.9.0

To install packages from the [JSR](https://jsr.io/) registry, use the `jsr:` protocol prefix:

```
pnpm add jsr:@hono/hono
pnpm add jsr:@hono/hono@4
pnpm add jsr:@hono/hono@latest
```

This works just like installing from npm, but tells pnpm to resolve the package through JSR instead.

### crates.io and PyPI

Added in: v12.4.0 (pnpm v12 only)

A `crate:` or `pypi:` prefix asks for a package of another ecosystem, once that ecosystem is enabled in `pnpm-workspace.yaml`:

```sh
pnpm add crate:serde
pnpm add pypi:httpx
```

Such a dependency is recorded in that ecosystem's own manifest and lockfile, not in `package.json` or `pnpm-lock.yaml`. See [Cargo dependencies](./cargo.md) and [Python dependencies](./python.md).

### Named registries

Added in: v11.1.0

A [named registry](./settings/dependency-resolution.md#namedregistries) alias resolves a package against a specific registry, regardless of the default one:

```sh
pnpm add work:@corp/lib@^2.0.0
pnpm add gh:@my-org/private-pkg
pnpm add npmjs:left-pad
```

`gh:` (GitHub Packages) and, since v11.20.0, `npmjs:` (the public npm registry) work without configuration. Any other alias must be mapped under [`namedRegistries`](./settings/dependency-resolution.md#namedregistries) in `pnpm-workspace.yaml` or, since v11.11.0, in the [global configuration file](./cli/config.md) (`config.yaml`).

### Workspace

Note that when adding dependencies and working within a [workspace], packages
will be installed from the configured sources, depending on whether or not
[`linkWorkspacePackages`] is set, and use of the
[`workspace: range protocol`].

[workspace]: ./workspaces.md
[`linkWorkspacePackages`]: ./workspaces.md#linkworkspacepackages
[`workspace: range protocol`]: ./workspaces.md#workspace-protocol-workspace

### Local file system

There are two ways to install from the local file system:

1. from a tarball file (`.tar`, `.tar.gz`, or `.tgz`)
2. from a directory

Examples:

```sh
pnpm add ./package.tar.gz
pnpm add ./some-directory
```

When you install from a directory, a symlink will be created in the current
project's `node_modules`, so it is the same as running `pnpm link`.

## Exotic sources

Exotic sources are useful for development but may pose supply chain risks when used by transitive dependencies.

### Remote tarball

The argument must be a fetchable URL starting with "http://" or "https://".

Example:

```sh
pnpm add https://github.com/indexzero/forever/tarball/v0.5.6
```

### Git repository

```sh
pnpm add <git remote url>
```

Installs the package from the Git repository at the given URL. Depending on the repository, pnpm either downloads a source archive from the Git host or clones the repository with Git — see [how Git dependencies are resolved](#how-git-dependencies-are-resolved).

You may install packages from Git by:

* Latest commit from default branch:
```
pnpm add kevva/is-positive
```
* Git commit hash:
```
pnpm add kevva/is-positive#97edff6f525f192a3f83cea1944765f769ae2678
```
* Git branch:
```
pnpm add kevva/is-positive#master
```
* Git branch relative to refs:
```
pnpm add zkochan/is-negative#heads/canary
```
* Git tag:
```
pnpm add zkochan/is-negative#2.0.1
```
* V-prefixed Git tag:
```
pnpm add andreineculau/npm-publish-git#v0.0.7
```

#### Install from a Git repository using semver

You can specify version (range) to install using the `semver:` parameter. For example:

* Strict semver:
```
pnpm add zkochan/is-negative#semver:1.0.0
```
* V-prefixed strict semver:
```
pnpm add andreineculau/npm-publish-git#semver:v0.0.7
```
* Semver version range:
```
pnpm add kevva/is-positive#semver:^2.0.0
```
* V-prefixed semver version range:
```
pnpm add andreineculau/npm-publish-git#semver:<=v0.0.7
```

#### Install from a subdirectory of a Git repository

You may also install just a subdirectory from a Git-hosted monorepo using the `path:` parameter. For instance:

```
pnpm add RexSkz/test-git-subfolder-fetch#path:/packages/simple-react-app
```

#### Install from a Git repository via a full URL

If you want to be more explicit or are using alternative Git hosting, you might want to spell out full Git URL:

```
# git+ssh
pnpm add git+ssh://git@github.com:zkochan/is-negative.git#2.0.1

# https
pnpm add https://github.com/zkochan/is-negative.git#2.0.1
```

#### Install from a Git repository using hosting providers shorthand

You can use a protocol shorthand `[provider]:` for certain Git providers:

```
pnpm add github:zkochan/is-negative
pnpm add bitbucket:pnpmjs/git-resolver
pnpm add gitlab:pnpm/git-resolver
```

If `[provider]:` is omitted, it defaults to `github:`.

#### Install from a Git repository combining different parameters

It is possible to combine multiple parameters by separating them with `&`. This can be useful for forks of monorepos:

```
pnpm add RexSkz/test-git-subdir-fetch.git#beta\&path:/packages/simple-react-app
```

Installs from the `beta` branch and only the subdirectory at `/packages/simple-react-app`.

#### How Git dependencies are resolved

Added in: v12.0.0 (pnpm v12 only)

For repositories on GitHub, GitLab, and Bitbucket, the specifier is an **identity**, not a choice of transport. All of the following name the same dependency and resolve identically:

```
kevva/is-positive
github:kevva/is-positive
git+https://github.com/kevva/is-positive.git
https://github.com/kevva/is-positive.git
git+ssh://git@github.com/kevva/is-positive.git
```

Each resolves through the host's canonical HTTPS URL, and the lockfile records one of two shapes:

* the **host's source archive**, a plain tarball download. This is recorded only when an anonymous request for that exact archive URL succeeds, so a recorded archive URL is fetchable by construction.
* otherwise a **`git` resolution over the canonical HTTPS URL**, which every machine that has access to the repository can fetch.

pnpm never records an SSH URL for these hosts. Which transport a given machine uses to reach the host is that machine's Git configuration, not a property of the project.

pnpm 11 keeps the specifier's transport as part of the recorded URL, but since v11.21.0 it records an SSH URL only when the specifier itself asks for one (`git+ssh://` or `git@host:...`): a shorthand like `owner/repo` resolves and records over HTTPS, so a lockfile written on a machine with SSH keys still installs on a CI runner without them. An SSH URL recorded by an older pnpm can be re-recorded over HTTPS with `pnpm update <package>`.

##### Using SSH for private repositories

Configure the rewrite in Git itself, on the machine:

```sh
git config --global url."git@github.com:".insteadOf https://github.com/
```

pnpm shells out to `git`, so the rewrite applies to all of pnpm's Git operations automatically. The same holds on CI: give the runner an SSH key and this rewrite, or an HTTPS credential helper — the lockfile is identical either way.

##### Repositories on other hosts

A URL that does not point at a known host (a self-hosted GitLab, Gitea, or any internal Git server) is kept exactly as written, transport included — for those, the URL *is* the identity. URLs with credentials embedded in them are also kept verbatim, and never resolve to a host archive.

:::info

In pnpm v11 and earlier, resolution probed the network to decide between HTTPS and SSH. That could record whichever transport happened to work on the machine that ran the install — most often an `ssh://` URL that then failed on CI runners without SSH keys. pnpm v12 removes the probing. Existing lockfile entries are left untouched; the rules above apply when an entry is added or re-resolved.

:::

#### How a Git dependency is built

Added in: v12.0.0-rc.6 (pnpm v12 only)

A Git-hosted dependency that ships sources rather than a built package has to be prepared — its dependencies installed and its build script run — before it can be installed. pnpm prepares it with the package manager the dependency itself asks for, instead of assuming the host has the right one:

* the dependency's own `packageManager` / [`devEngines.packageManager`](./package_json.md#devenginespackagemanager) pin wins — that is what its authors test against;
* failing a pin, the lockfile it ships names the package manager. A `yarn.lock` also names *which* Yarn line: Yarn Berry stamps `__metadata:` into every lockfile it writes and neither line can read the other's, so a Classic lockfile is no longer installed by Berry.

pnpm [provides that package manager](./package-managers.md) when the dependency pinned a version, or when the host cannot satisfy what the dependency needs. A repository built with Yarn therefore installs on a machine that has only pnpm, while a host that already has a suitable package manager — under every name the build may invoke — keeps using its own.
