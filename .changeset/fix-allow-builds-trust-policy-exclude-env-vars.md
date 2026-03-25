---
"@pnpm/config.reader": minor
"pnpm": minor
---

Fixed `allowBuilds` and `trustPolicyExclude` not being forwarded as environment variables to child pnpm processes when running scripts.

Previously, `npm_config_trust_policy` and `npm_config_strict_dep_builds` were correctly forwarded via `npm_config_*` env vars, but `allowBuilds` (an object) and `trustPolicyExclude` (an array) were silently dropped because `@pnpm/npm-lifecycle` unconditionally skips array values and uses the wrong key name for non-rc object values.

The fix injects these as `pnpm_config_allow_builds` and `pnpm_config_trust_policy_exclude` (JSON-encoded) into `extraEnv`, using the same `pnpm_config_*` mechanism already used for `pnpm_config_verify_deps_before_run`. User-provided `pnpm_config_*` environment variables are preserved and not overridden.

Fixes [#10988](https://github.com/pnpm/pnpm/issues/10988).
