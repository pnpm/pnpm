---
"@pnpm/installing.deps-resolver": patch
"pnpm": patch
"pacquet": patch
---

An optional dependency written with a named-registry prefix, such as `work:^1.0.0`, now fails the install when it cannot be resolved but the lockfile already has a matching entry from that registry. Such a dependency was skipped, which removed its entries from the lockfile.
