---
"@pnpm/resolving.npm-resolver": patch
"@pnpm/resolving.resolver-base": patch
"@pnpm/store.controller-types": patch
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
---

Fixed `pnpm up -r <pkg>` downgrading the targeted package in unrelated consumers. Previously, when one consumer pinned the package to an older version (e.g. via an exact pin in its `package.json`), that older version was propagated as a preferred version for siblings, so consumers with looser ranges got pulled down to the older version instead of staying on `latest`. The resolver now distinguishes `updateRequested` (true only for packages that match the user's update target) from the broader `update` flag, and for the targeted package drops only the propagated exact-version pins — keeping install-time dedup for everything else, and keeping `range`/`tag` preferred-version selectors (such as the vulnerability-avoidance penalties from `pnpm audit --fix`) in effect even for the targeted package.
