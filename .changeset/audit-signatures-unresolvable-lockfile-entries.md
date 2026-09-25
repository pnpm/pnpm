---
"@pnpm/deps.compliance.audit": patch
"@pnpm/deps.compliance.commands": patch
"@pnpm/deps.security.signatures": patch
"pnpm": patch
---

`pnpm audit signatures` now reports lockfile entries that cannot be resolved to a package in the lockfile as invalid entries with a nonzero exit code, rather than silently dropping them [pnpm/pnpm#13638](https://github.com/pnpm/pnpm/issues/13638).
