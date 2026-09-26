---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

The lockfile verification error now suggests relaxing the policy that flagged an entry only if a fresh resolution still fails and you trust the affected packages. Errors from checks that no policy controls, such as a missing tarball integrity, no longer suggest relaxing a policy [#14411](https://github.com/pnpm/pnpm/issues/14411).
