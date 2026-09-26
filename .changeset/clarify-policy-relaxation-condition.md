---
"@pnpm/installing.deps-installer": patch
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
"pacquet": patch
---

The lockfile verification error now suggests relaxing the policy that flagged an entry only if a fresh resolution still fails and you trust the affected packages. Errors for a missing tarball integrity or a mismatched resolution shape no longer suggest relaxing a policy, because no policy controls them [#14411](https://github.com/pnpm/pnpm/issues/14411).
