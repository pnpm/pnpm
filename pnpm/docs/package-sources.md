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

To install packages from the [JSR](https://jsr.io/) registry, use the `jsr:` protocol prefix:

```sh
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

### Provisioned tools

Added in: v12.13.0 (pnpm v12 only)

The `tool:` protocol asks for a tool pnpm downloads from the tool's own releases instead of from npm: Node.js, Bun, Deno, and Yarn.

```sh
pnpm add node@tool:24
pnpm add bun@tool:1.3.0
pnx node@tool:22 --version
pnx yarn@tool:4 --version
```

`yarn@tool:<version>` picks Yarn from wherever that line is published, like `yarn@<version>` does.

`runtime:` is an alias of `tool:` and keeps working everywhere it worked before. `pnpm-lock.yaml` still records these entries with `runtime:`, such as `node@runtime:24.6.0`, so lockfiles stay readable by earlier pnpm versions.

### Named registries

A [named registry](./settings/dependency-resolution.md#namedregistries) alias resolves a package against a specific registry, regardless of the default one:

```sh
pnpm add work:@corp/lib@^2.0.0
pnpm add gh:@my-org/private-pkg
pnpm add npmjs:left-pad
```

`gh:` (GitHub Packages) and `npmjs:` (the public npm registry) work without configuration. Any other alias must be mapped under [`namedRegistries`](./settings/dependency-resolution.md#namedregistries) in `pnpm-workspace.yaml` or in the [global configuration file](./cli/config.md) (`config.yaml`).

### Workspace

When adding dependencies and working within a [workspace], packages
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

`pnpm add ./some-directory` records the dependency with the `link:` protocol.
pnpm creates a symlink in the current project's `node_modules`, so it is the
same as running `pnpm link`.

To install a directory as a package instead, use the `file:` protocol, for
example `pnpm add file:./some-directory`. pnpm copies the directory into the
virtual store and installs its dependencies.

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

Installs the package from the Git repository at the given URL. Depending on the repository, pnpm either downloads a source archive from the Git host or clones the repository with Git. See [how Git dependencies are resolved](#how-git-dependencies-are-resolved).

You may install packages from Git by:

* Latest commit from default branch:
```sh
pnpm add kevva/is-positive
```
* Git commit hash:
```sh
pnpm add kevva/is-positive#97edff6f525f192a3f83cea1944765f769ae2678
```
* Git branch:
```sh
pnpm add kevva/is-positive#master
```
* Git branch relative to refs:
```sh
pnpm add zkochan/is-negative#heads/canary
```
* Git tag:
```sh
pnpm add zkochan/is-negative#2.0.1
```
* V-prefixed Git tag:
```sh
pnpm add andreineculau/npm-publish-git#v0.0.7
```

#### Install from a Git repository using semver

You can specify version (range) to install using the `semver:` parameter. For example:

* Strict semver:
```sh
pnpm add zkochan/is-negative#semver:1.0.0
```
* V-prefixed strict semver:
```sh
pnpm add andreineculau/npm-publish-git#semver:v0.0.7
```
* Semver version range:
```sh
pnpm add kevva/is-positive#semver:^2.0.0
```
* V-prefixed semver version range:
```sh
pnpm add andreineculau/npm-publish-git#semver:<=v0.0.7
```

#### Install from a subdirectory of a Git repository

You may also install just a subdirectory from a Git-hosted monorepo using the `path:` parameter. For instance:

```sh
pnpm add RexSkz/test-git-subfolder-fetch#path:/packages/simple-react-app
```

#### Install from a Git repository via a full URL

If you want to be more explicit or are using alternative Git hosting, you might want to spell out full Git URL:

```sh
# git+ssh
pnpm add git+ssh://git@github.com:zkochan/is-negative.git#2.0.1

# https
pnpm add https://github.com/zkochan/is-negative.git#2.0.1
```

#### Install from a Git repository using hosting providers shorthand

You can use a protocol shorthand `[provider]:` for certain Git providers:

```sh
pnpm add github:zkochan/is-negative
pnpm add bitbucket:pnpmjs/git-resolver
pnpm add gitlab:pnpm/git-resolver
pnpm add gist:106cfef49f8476b753a9cbbf9c65aff1
```

If `[provider]:` is omitted, it defaults to `github:`.

#### Install from a Git repository combining different parameters

It is possible to combine multiple parameters by separating them with `&`. This can be useful for forks of monorepos:

```sh
pnpm add RexSkz/test-git-subdir-fetch.git#beta\&path:/packages/simple-react-app
```

Installs from the `beta` branch and only the subdirectory at `/packages/simple-react-app`.

#### How Git dependencies are resolved

Added in: v12.0.0 (pnpm v12 only)

For repositories on GitHub, GitLab, Bitbucket, and GitHub Gist, the specifier is an **identity**, not a choice of transport. All of the following name the same dependency and resolve identically:

```text
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

##### Using SSH for private repositories

Configure the rewrite in Git itself, on the machine:

```sh
git config --global url."git@github.com:".insteadOf https://github.com/
```

pnpm shells out to `git`, so the rewrite applies to all of pnpm's Git operations automatically. The same holds on CI: give the runner an SSH key and this rewrite, or an HTTPS credential helper. The lockfile is identical either way.

##### Repositories on other hosts

A URL that does not point at a known host (a self-hosted GitLab, Gitea, or any internal Git server) is kept exactly as written, transport included. For those, the URL *is* the identity. URLs with credentials embedded in them are also kept verbatim, and never resolve to a host archive.

:::info

Existing lockfile entries are left untouched. The rules above apply when an entry is added or re-resolved, and `pnpm update <package>` re-records an SSH URL over HTTPS.

:::

#### How a Git dependency is built

Added in: v12.0.0-rc.6 (pnpm v12 only)

A Git-hosted dependency that ships sources rather than a built package has to be prepared (its dependencies installed and its build script run) before it can be installed. pnpm prepares it with the package manager the dependency asks for. See [Preparing a git-hosted dependency](./package-managers.md#preparing-a-git-hosted-dependency).
