# @pnpm/releasing.versioning

## 1100.1.0

### Minor Changes

- Added `versioning.epics` to `pnpm-workspace.yaml`. An epic ties a group of member packages to a lead package, constraining every member's major version to a band derived from the lead's major: while the lead is on major `M`, members live in `M*100 … M*100+99`. Members move independently inside the band (patch, minor, and a `major` intent that stays in-band); a bump that would carry a member past the band ceiling is rejected until the lead advances its own major. When a release plan takes the lead to a new stable major, every member re-bases to the band floor in the same plan. Membership is matched with pnpm's package selectors — name globs, `./`-prefixed directory globs, and `!`-prefixed negations.

- Added native workspace release management [#12952](https://github.com/pnpm/pnpm/issues/12952): the new `pnpm change` command records change intents as changesets-compatible `.changeset/*.md` files (`pnpm change status` shows the pending release plan), and the bare `pnpm version -r` consumes them — bumping versions across the workspace with dependent propagation through `workspace:` ranges, fixed groups, a `maxBump` cap, `--filter` narrowing, and `--dry-run` — writing changelogs, and recording consumed intents in a committed ledger that keeps cherry-picks and merge-backs between release branches safe. Packages can be moved onto per-package release lanes with the new `pnpm lane <name> --filter <pkg>` command and back with `pnpm lane main --filter <pkg>` (`pnpm lane` shows the membership), releasing `X.Y.Z-lane.N` prereleases from the same runs that release stable versions of the packages on the main lane. Configuration lives under the new `versioning` key of `pnpm-workspace.yaml` (`fixed`, `ignore`, `maxBump`, `lanes`, `changelog`). When two workspace projects publish the same name, intent files, `versioning.lanes`, and `versioning.fixed`/`ignore` may reference a project by its workspace-relative directory path (e.g. `"./pnpm/npm/pnpm"`) — the one additive extension to the changesets format, applied automatically by `pnpm change`.

  Release changelogs default to `registry` storage (`versioning.changelog.storage`): no `CHANGELOG.md` is committed. Each release's section is composed at publish time and packed into the published tarball on top of the previously published version's changelog, and the consumed change intents are garbage-collected by a later `pnpm version -r` only once the registry confirms the version is published with its section. Set `versioning.changelog.storage: repository` to keep committed `CHANGELOG.md` files instead.

### Patch Changes

- Updated dependencies:
  - @pnpm/types@1101.4.0
  - @pnpm/workspace.project-manifest-reader@1100.0.15
