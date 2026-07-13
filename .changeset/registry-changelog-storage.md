---
"@pnpm/releasing.versioning": minor
"@pnpm/releasing.commands": minor
"pnpm": minor
"pacquet": minor
---

Native workspace release changelogs now default to `registry` storage: no `CHANGELOG.md` is committed to the repository. At publish time `pnpm publish` (and `pnpm pack`) composes the release's changelog section from its parked change intents, fetches the previously published version's tarball, prepends the new section to its `CHANGELOG.md`, and packs the result into the new tarball — chaining onto the highest published version that is semver-lower than the one being published. Consumed change intents are garbage-collected by a later `pnpm version -r` only once the registry confirms the version is published with its composed section, so the repository is never left as the only copy of unpublished prose. Set `versioning.changelog.storage: repository` in `pnpm-workspace.yaml` to keep committed `CHANGELOG.md` files instead. Implements the changelog-storage design of the native monorepo versioning RFC (https://github.com/pnpm/rfcs/blob/main/text/0006-monorepo-versioning.md).
