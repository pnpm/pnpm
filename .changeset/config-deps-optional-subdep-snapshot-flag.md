---
"@pnpm/installing.env-installer": patch
"pnpm": patch
---

Mark optional subdependency snapshots of config dependencies with `optional: true` in the env lockfile, matching how optional dependencies are recorded elsewhere in `pnpm-lock.yaml`. Previously, snapshots for the platform-specific subdeps pulled in via a config dep's `optionalDependencies` were written as empty objects, which was inconsistent with the rest of the lockfile and made it look like those non-host platform variants were required.
