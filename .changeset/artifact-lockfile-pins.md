---
"@pnpm/lockfile.types": minor
"@pnpm/lockfile.fs": patch
"@pnpm/pnpr.client": minor
"@pnpm/installing.deps-resolver": patch
"@pnpm/installing.deps-restorer": minor
"@pnpm/installing.deps-installer": minor
"@pnpm/installing.commands": minor
"pnpm": minor
"pacquet": minor
---

Remote build artifacts are now pinned in `pnpm-lock.yaml` by input key, owner, and consumer platform after their signatures and contents are verified. Frozen installs enforce existing pins without changing them. Run `pnpm install --refresh-artifact-pins` to explicitly replace the recorded pins ([pnpm/pnpm#13771](https://github.com/pnpm/pnpm/issues/13771)).
