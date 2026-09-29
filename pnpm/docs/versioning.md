---
id: versioning
title: Release management
---

Added in: v11.13.0

pnpm can version and release a workspace on its own, without a separate release tool. The workflow has two halves:

1. As you work, [`pnpm change`](./cli/change.md) records **change intents** — small markdown files in `.changeset/` saying which packages a change affects, how each should be bumped, and a summary that becomes the changelog entry. These are committed alongside the change.
2. At release time, the bare [`pnpm version -r`](./cli/version.md#recursive-releases) consumes the pending intents: it bumps versions across the workspace, propagates to dependents, writes changelogs, and records what it consumed in a committed ledger.

Intent files use the [changesets](https://github.com/changesets/changesets) format, so an existing `.changeset/` directory keeps working. See [Using Changesets with pnpm](./using-changesets.md) if you would rather keep using the Changesets CLI.

## Recording a change

```sh
pnpm change
```

This prompts for the affected packages, their bump types, and a summary, then writes a file like `.changeset/calm-cats-resolve.md`:

```markdown
---
"@example/core": minor
---

Added a `--watch` flag to the build command.
```

To see what the pending intents would produce:

```sh
pnpm change status
```

## Releasing

```sh
pnpm version -r
```

This applies the release plan: every package named by an intent is bumped, and so is every package that depends on it through a `workspace:` range. Preview it first with `--dry-run`, and narrow it with `--filter`.

Because a recursive run can bump many packages to different versions, no git commit or tag is created — there is no single version to tag. Commit the result yourself, then publish with `pnpm publish -r`.

### First releases

Since v11.16.0, the first release of a package publishes the version written in its manifest verbatim, instead of bumping off it. `pnpm version -r` and `pnpm change status` check the registry for each release's current version; when that version is not yet published, the package debuts at it and its pending change intents apply only from the next release. A newly added package seeded at `1100.0.0` with a `minor` intent is therefore published as `1100.0.0` rather than skipping straight to `1100.1.0`.

## Configuration

Release behavior is configured under the `versioning` key of `pnpm-workspace.yaml`:

```yaml title="pnpm-workspace.yaml"
versioning:
  fixed:
    - ['@example/cli', '@example/napi']
  ignore:
    - '@example/internal'
  maxBump: minor
  lanes:
    '@example/cli': alpha
  changelog:
    storage: repository
```

Every key is described in [Versioning Settings](./settings/versioning.md).

Where two workspace projects publish the same name, a project can be referenced by its workspace-relative directory instead of its name, with a `./` prefix (e.g. `"./packages/cli"`). This works in intent files and in `versioning.lanes`, `versioning.fixed`, and `versioning.ignore`.

### Fixed groups

Packages listed together in `versioning.fixed` always release at one shared version — the highest current version in the group, bumped by the largest bump any member needs. A fixed group must move between lanes together, and must sit entirely inside or entirely outside an epic.

### Lanes

A lane is a parallel release track. While a package is on one, it releases `X.Y.Z-<lane>.N` prereleases from the same runs that release stable versions of everything on the main lane. This lets a rewrite or a major-version line bake in public while the rest of the workspace keeps shipping.

```sh
pnpm lane alpha --filter @example/cli   # move onto the alpha lane
pnpm lane main --filter @example/cli    # graduate back to stable
pnpm lane                               # show membership
```

See [`pnpm lane`](./cli/lane.md) for how the prerelease versions are computed.

### Epics

An epic ties a group of member packages to a lead package, constraining every member's major version to a band derived from the lead's major: while the lead is on major `M`, members live in `M*100` … `M*100+99`.

```yaml title="pnpm-workspace.yaml"
versioning:
  epics:
    - lead: '@example/app'
      packages:
        - './packages/**'
        - '!./packages/private-*'
```

With the lead on `11.x`, members occupy majors `1100`–`1199`. Members move independently inside the band — patch, minor, and even a `major` intent that stays in-band. A bump that would carry a member past the band ceiling is rejected until the lead advances its own major. When a release plan takes the lead to a new stable major, every member re-bases to the band floor in the same plan.

Membership is matched with pnpm's package selectors: name globs, `./`-prefixed directory globs, and `!`-prefixed negations. Selectors are evaluated in order and the last one to match decides, so a later include can re-admit a package an earlier negation excluded. The lead is never a member of its own band.

## Checking committed versions

[`pnpm change check`](./cli/change.md#check) validates the versions already committed in the workspace against the `versioning.epics` bands and the `versioning.fixed` groups, without reading any change intent. Run it in CI, next to the lint and test steps, so a version that drifted out of its band is caught on the pull request that introduced it rather than at release time.

## Changelogs

By default (`versioning.changelog.storage: registry`) no `CHANGELOG.md` is committed. Each release's section is composed at publish time and packed into the published tarball on top of the previously published version's changelog. Consumed change intents are garbage-collected by a later `pnpm version -r` only once the registry confirms the version was published with its section.

Set `versioning.changelog.storage: repository` to keep committed `CHANGELOG.md` files in every package instead.

## The ledger

`pnpm version -r` records every consumed intent in `.changeset/ledger.yaml`, a committed, append-only file:

```yaml
"@example/core@1.3.0":
  dir: packages/core
  intents:
    - calm-cats-resolve
```

Consumption is tracked per project: an intent file is deleted only once every project it names has released. This is what makes cherry-picks and merge-backs between release branches safe — a release branch that has already consumed an intent will not consume it again when the commit is merged forward, and a package on a lane can half-consume an intent whose prose still has a stable release to compose.
