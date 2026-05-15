---
"@pnpm/deps.graph-builder": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
---

Fix `TypeError: Cannot use 'in' operator to search for ... in undefined` during `pnpm install --frozen-lockfile` and `pnpm deploy` when a peer-dep variant snapshot in `pnpm-lock.yaml` omits its `resolution` field. Peer-dep variants legally inherit `resolution` from the base entry — the lockfile writer omits it on variants to avoid duplication — but multiple readers (`buildGraphFromPackages`, `pkgSnapshotToResolution`, `convertPackageSnapshot`) accessed `pkgSnapshot.resolution` without guarding. Inherit the resolution from the base entry where available, or synthesize a directory resolution from the depPath's `file:` prefix when the base has been pruned (e.g. by `turbo prune --docker`).
