---
"@pnpm/lockfile.fs": patch
"@pnpm/exec.lifecycle": patch
"pnpm": patch
---

Fix two crashes with `injectWorkspacePackages: true` when the lockfile has been pruned (e.g. by `turbo prune --docker`):

- `Cannot use 'in' operator to search for 'directory' in undefined`: a peer-dependency-variant injected snapshot inherits its `resolution` from the base `packages:` entry; when a pruner drops that base entry the readers crash. `convertToLockfileObject` now reconstructs the directory resolution from the `file:` depPath at load time — a single normalization point, so every reader sees a fully-formed snapshot.
- `ERR_PNPM_ENOENT` on `node_modules/.bin/<tool>`: after `prepare`/`postinstall`, `runLifecycleHooksConcurrently` re-imported each injected workspace package; the `scanDir`-into-`filesMap` workaround fed target-internal paths to the importer, which the `makeEmptyDir` fast path (#11088) then wiped. Drop the workaround and pass `keepModulesDir: true` so the importer preserves the target's existing `node_modules` (bin links + transitive deps) and source files keep their hardlinks.
