---
"pacquet": patch
---

Engine checks (`engines.node` / `engines.pnpm`) now match npm-semver's `includePrerelease` semantics exactly: a prerelease version no longer satisfies a fully specified `>=` bound (`9.0.0-alpha.1` does not satisfy `>=9.0.0`), while still satisfying expanded ranges like `9`, `>=9`, and `^9.0.0`.
