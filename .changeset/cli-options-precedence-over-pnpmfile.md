---
"pnpm": patch
"pacquet": patch
---

Fixed an issue where `.pnpmfile.mjs` / `.pnpmfile.cjs` `updateConfig` hooks could override explicit CLI flags (such as `--registry`). CLI options now consistently take precedence over pnpmfile config overrides [`pnpm/pnpm#14063`](https://github.com/pnpm/pnpm/issues/14063).
