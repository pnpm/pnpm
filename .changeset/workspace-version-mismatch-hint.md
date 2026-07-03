---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

When a dependency cannot be found in the registry (404) or the registry has no matching version, and a workspace project with the same name exists only at non-matching versions, the error now reports the available workspace versions (`ERR_PNPM_NO_MATCHING_VERSION_INSIDE_WORKSPACE`) instead of the raw registry failure [pnpm/pnpm#1379](https://github.com/pnpm/pnpm/issues/1379). Other registry failures (authorization, network, server errors) still propagate unchanged. The pacquet (Rust) resolver applies the same behavior.
