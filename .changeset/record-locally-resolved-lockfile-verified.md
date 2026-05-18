---
"@pnpm/installing.deps-installer": patch
"@pnpm/lockfile.fs": minor
"pnpm": patch
---

Record the post-resolution lockfile in the verification cache. Previously the cache only captured the lockfile that was loaded at the start of an install, so a flow like `pnpm install <pkg>` followed by `rm -rf node_modules && pnpm install` re-ran the per-package registry round-trip against the newly written lockfile even though the local resolver had already enforced the policy when picking those versions. The fresh lockfile is now recorded immediately after each install-time write, so the second install takes the cache fast path.
