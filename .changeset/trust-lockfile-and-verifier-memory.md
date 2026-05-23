---
"@pnpm/config": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.commands": minor
"@pnpm/resolving.npm-resolver": patch
"pnpm": minor
---

Added a new setting `trustLockfile`. When `true`, `pnpm install` skips the supply-chain verification pass that re-applies `minimumReleaseAge` / `trustPolicy='no-downgrade'` to every entry in the loaded lockfile. The install treats the lockfile as already-trusted — useful for closed-source projects where every commit comes from a trusted author, or for CI runs against an already-verified lockfile. Defaults to `false`; verification stays on by default. Set in `pnpm-workspace.yaml`.

Also cut the memory footprint of the verification pass itself: the per-(registry, name) trust-meta cache previously retained the full packument — dependency graphs, scripts, README, and per-version manifests — for the entire install. On large workspaces (`~4k` lockfile entries with `minimumReleaseAge` + `trustPolicy: no-downgrade` enabled) this could OOM CI runners with a 2GB heap cap. The cache now stores only the fields the trust check actually reads (`time`, per-version `_npmUser.trustedPublisher`, `dist.attestations.provenance`). The abbreviated-metadata cache is similarly projected to just the package-level `modified` field and the set of currently-listed version names. Fixes [#11860](https://github.com/pnpm/pnpm/issues/11860).
