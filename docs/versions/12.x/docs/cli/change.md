---
id: change
title: pnpm change
---

Added in: v11.13.0

Records a change intent: which packages a change affects, the bump type for each, and a summary that becomes the changelog entry. The intent file is written to `.changeset/` in the [changesets](https://github.com/changesets/changesets) format.

```sh
pnpm change [--bump <type>] [--summary <text>] [<pkg>...]
pnpm change status
pnpm change check
```

Change intents are consumed later by [`pnpm version -r`](./version.md#recursive-releases). See [Release management](../versioning.md) for the whole workflow.

## Usage

Run without arguments to record an intent interactively:

```sh
pnpm change
```

You are asked three questions:

1. **Which packages does this change affect?** Packages changed since the merge base with `main` (or `master`) are preselected.
2. **Which packages should have a major bump?**, then the same for `minor`. Anything left over is bumped as `patch`.
3. **Summary of the change**, which becomes the changelog entry.

The result is a file such as `.changeset/calm-cats-resolve.md`:

```markdown
---
"@example/core": minor
"@example/cli": patch
---

Added a `--watch` flag to the build command.
```

Commit this file along with your change.

Passing package names together with `--bump` and `--summary` records an intent without prompting, which is useful in scripts:

```sh
pnpm change --bump patch --summary "Fixed a crash on empty input" @example/core
```

## Subcommands

### status

Show the pending change intents and the release plan they produce.

```sh
pnpm change status
```

```
Pending change intents:
  .changeset/calm-cats-resolve.md

Release plan:
  @example/core: 1.2.0 → 1.3.0 (minor, via intent)
  @example/cli: 0.4.1 → 0.4.2 (patch, via intent+dependencies)
```

The cause of each bump is one of `intent` (a change intent named the package), `dependencies` (a dependent was pulled in by propagation), `fixed` (a [fixed group](../versioning.md#fixed-groups) companion), or `epic` (an [epic](../versioning.md#epics) re-base).

### check

Added in: v12.4.0

Validate the versions committed in the workspace against the [epic](../versioning.md#epics) bands and [fixed group](../versioning.md#fixed-groups) lockstep declared in `pnpm-workspace.yaml`.

```sh
pnpm change check
```

Unlike `pnpm change status`, which reports the release the pending intents produce, `check` looks only at the versions already in the manifests. It reads no intents, so it is the step to run on every pull request: a package whose version drifted out of its epic's band, or out of step with its fixed group, fails the command. Every violation is listed, including ones in packages the current release does not touch.

## Options

### --bump &lt;type&gt;

The bump type for the named packages: `none`, `patch`, `minor`, or `major`. `none` records an explicit decline — the change needs no release.

### --summary &lt;text&gt;

The summary for the changelog entry. Together with package names, this runs the command non-interactively.

## Referencing packages by directory

When two workspace projects publish the same name, a package can be referenced by its workspace-relative directory instead, with a `./` prefix:

```markdown
---
"./packages/cli": minor
---
```

This is the one additive extension pnpm makes to the changesets format. `pnpm change` writes it automatically when a name is ambiguous.
