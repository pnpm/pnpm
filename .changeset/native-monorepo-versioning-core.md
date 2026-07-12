---
"@pnpm/types": minor
"@pnpm/config.reader": minor
"@pnpm/workspace.workspace-manifest-reader": minor
"@pnpm/releasing.versioning": minor
"@pnpm/releasing.commands": minor
"pnpm": minor
---

Added native workspace release management [#12952](https://github.com/pnpm/pnpm/issues/12952): the new `pnpm change` command records change intents as changesets-compatible `.changeset/*.md` files (`pnpm change status` shows the pending release plan), and the bare `pnpm version -r` consumes them — bumping versions across the workspace with dependent propagation through `workspace:` ranges, fixed groups, a `maxBump` cap, `--filter` narrowing, `--dry-run`, and `--snapshot` releases — writing changelogs, and recording consumed intents in a committed ledger that keeps cherry-picks and merge-backs between release branches safe. Packages can be placed on per-package prerelease lines with `pnpm version unstable <tag> --filter <pkg>` and moved back with `pnpm version stable --filter <pkg>`, releasing `X.Y.Z-tag.N` versions from the same runs that release stable versions of other packages. Configuration lives under the new `versioning` key of `pnpm-workspace.yaml` (`fixed`, `ignore`, `maxBump`, `prereleases`, `changelog`).
