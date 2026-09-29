---
id: package-sources
title: Supported package sources
---

pnpm supports installing packages from various sources. These sources are divided into two categories: **trusted sources** and **exotic sources**.

Exotic sources (like Git repositories or direct tarball URLs) can introduce supply chain risks when used by transitive dependencies. You can prevent transitive dependencies from using exotic sources by setting [`blockExoticSubdeps`] to `true`.

[`blockExoticSubdeps`]: ./settings.md#blockexoticsubdeps

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

### Workspace

Note that when adding dependencies and working within a [workspace], packages
will be installed from the configured sources, depending on whether or not
[`linkWorkspacePackages`] is set, and use of the
[`workspace: range protocol`].

[workspace]: ./workspaces.md
[`linkWorkspacePackages`]: ./settings.md#linkworkspacepackages
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

Installs the package from the hosted Git provider, cloning it with Git.

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
