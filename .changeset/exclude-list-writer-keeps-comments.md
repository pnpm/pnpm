---
"pacquet": patch
---

`pnpm audit --fix` and the `minimumReleaseAgeStrict` approval prompt no longer drop the comments of `minimumReleaseAgeExclude` when they append an entry to the list in `pnpm-workspace.yaml`. The rest of the list is now left as written.

The `trustPolicyExcludePrune` and `minimumReleaseAgeExcludePrune` cleanups leave the comments of the entries they keep in place, too.
