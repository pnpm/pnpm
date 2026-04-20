---
"@pnpm/deps.graph-hasher": minor
"@pnpm/resolving.resolver-base": minor
"@pnpm/installing.deps-installer": patch
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-restorer": patch
"@pnpm/installing.package-requester": patch
"@pnpm/building.after-install": patch
"@pnpm/deps.graph-builder": patch
"pnpm": patch
---

Fix: different platform variants of the same runtime (e.g. `node@runtime:25.9.0` glibc vs. musl) no longer share a single global-virtual-store entry. The virtual store path now incorporates the selected variant's integrity, so installs with different `--os`/`--cpu`/`--libc` end up in separate directories and `pnpm add --libc=musl node@runtime:<v>` reliably fetches the musl binary even when the glibc variant is already cached.
