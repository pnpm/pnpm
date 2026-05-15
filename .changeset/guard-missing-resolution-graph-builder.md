---
"@pnpm/lockfile.utils": minor
"@pnpm/deps.graph-builder": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/releasing.commands": patch
"@pnpm/deps.compliance.sbom": patch
"@pnpm/deps.inspection.tree-builder": patch
"pnpm": patch
---

Fix `TypeError: Cannot use 'in' operator to search for ... in undefined` during `pnpm install --frozen-lockfile`, `pnpm install --node-linker=hoisted`, `pnpm deploy`, `pnpm list`, and `pnpm sbom` when a peer-dep variant snapshot in `pnpm-lock.yaml` omits its `resolution` field. Peer-dep variants legally inherit `resolution` from the base entry — the lockfile writer omits it on variants to avoid duplication — but every reader that iterates `lockfile.packages` and dereferences `pkgSnapshot.resolution` (`buildGraphFromPackages` for the isolated linker, `lockfileToHoistedDepGraph` for the hoisted linker, `convertPackageSnapshot` for deploy, `collectSbomComponents` for SBOM, `getPkgInfo` for the tree/list view) had the same latent crash. Introduce a shared `inheritOrSynthesizeResolution` helper in `@pnpm/lockfile.utils` that inherits the resolution from the base entry where available, or synthesizes a directory resolution from the depPath's `file:` prefix when the base has been pruned (e.g. by `turbo prune --docker`). Call it at every iteration site so downstream code paths — including `pkgSnapshotToResolution` — see fully-formed snapshots.
