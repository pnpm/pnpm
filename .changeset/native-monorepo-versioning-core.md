---
"@pnpm/types": minor
"@pnpm/config.reader": minor
"@pnpm/workspace.workspace-manifest-reader": minor
"@pnpm/releasing.versioning": minor
"@pnpm/releasing.commands": minor
"pnpm": minor
---

Added native workspace release management [#12952](https://github.com/pnpm/pnpm/issues/12952): the new `pnpm change` command records change intents as changesets-compatible `.changeset/*.md` files (`pnpm change status` shows the pending release plan), and the bare `pnpm version -r` consumes them — bumping versions across the workspace with dependent propagation through `workspace:` ranges, fixed groups, a `maxBump` cap, `--filter` narrowing, and `--dry-run` — writing changelogs, and recording consumed intents in a committed ledger that keeps cherry-picks and merge-backs between release branches safe. Packages can be moved onto per-package release lanes with the new `pnpm lane <name> --filter <pkg>` command and back with `pnpm lane main --filter <pkg>` (`pnpm lane` shows the membership), releasing `X.Y.Z-lane.N` prereleases from the same runs that release stable versions of the packages on the main lane. Configuration lives under the new `versioning` key of `pnpm-workspace.yaml` (`fixed`, `ignore`, `maxBump`, `lanes`, `changelog`). When two workspace projects publish the same name, intent files, `versioning.lanes`, and `versioning.fixed`/`ignore` may reference a project by its workspace-relative directory path (e.g. `"./pnpm/npm/pnpm"`) — the one additive extension to the changesets format, applied automatically by `pnpm change`.

Release changelogs default to `registry` storage (`versioning.changelog.storage`): no `CHANGELOG.md` is committed. Each release's section is composed at publish time and packed into the published tarball on top of the previously published version's changelog, and the consumed change intents are garbage-collected by a later `pnpm version -r` only once the registry confirms the version is published with its section. Set `versioning.changelog.storage: repository` to keep committed `CHANGELOG.md` files instead.
