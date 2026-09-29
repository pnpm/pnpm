---
id: version
title: pnpm version
---

Added in: v11.0.0

Bump the package version.

```sh
pnpm version <newversion>
pnpm version <major|minor|patch|premajor|preminor|prepatch|prerelease|from-git>
pnpm version -r [--dry-run]
```

`<newversion>` can be any of the bump types above or an explicit semver version (e.g. `1.2.3`). Workspaces and the `workspace:` protocol are supported, so cross-references between workspace packages are updated correctly.

When run inside a git repository, `pnpm version` creates a git commit and an annotated tag for the bump. The working tree must be clean (see `--no-git-checks` below) and commits/tags can be disabled with `--no-git-tag-version`. Git commits and tags are always skipped in recursive mode because multiple packages may be bumped to different versions in a single run.

## Usage

```sh
pnpm version patch
pnpm version minor
pnpm version major
pnpm version 2.0.0
pnpm version prerelease --preid beta
```

## Recursive releases

Added in: v11.13.0

Run with `-r` and **no version argument** to consume the pending change intents recorded by [`pnpm change`](./change.md):

```sh
pnpm version -r
```

This assembles a release plan from the `.changeset/*.md` intent files and applies it: every package named by an intent is bumped, and so is every package that depends on it through a `workspace:` range. It then writes changelogs and records the consumed intents in `.changeset/ledger.yaml`.

Preview the plan without touching anything:

```sh
pnpm version -r --dry-run
```

Narrow it to part of the workspace with `--filter`. The selection is expanded until it settles, so fixed-group companions and dependents whose ranges the bump invalidates are pulled in automatically.

The working tree must be clean unless `--dry-run` or `--no-git-checks` is passed. See [Release management](../versioning.md) for the full workflow.

## Options

### --preid &lt;prerelease-id\>

The "prerelease identifier" to use as a prefix for the prerelease part of a semver.

```sh
pnpm version prerelease --preid beta
```

### --message, -m &lt;message\>

Commit message. Any `%s` in the message is replaced with the new version. Defaults to `%s`.

```sh
pnpm version patch --message "chore: release v%s"
```

### --tag-version-prefix &lt;prefix\>

Prefix used when creating the git tag. Defaults to `v` (e.g. `v1.2.3`). Set to an empty string to drop the prefix entirely.

### --no-git-tag-version

Do not create a git commit or tag for the version change.

### --no-commit-hooks

Skip git commit hooks (`--no-verify`) when committing the version bump.

### --sign-git-tag

Sign the generated git tag with GPG (`git tag -s`).

### --no-git-checks

Do not check whether the working tree is clean before bumping the version.

### --allow-same-version

Allow setting the version to the current version. This can be useful for CI pipelines.

### --recursive, -r

Apply the version bump to every package in the workspace (optionally narrowed with `--filter`). Git commit and tag creation are skipped in recursive mode.

### --json

Output the list of bumped packages in JSON format.
