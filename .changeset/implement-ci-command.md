---
'pnpm': minor
---

Implement `pnpm ci` command for clean installs [#6100](https://github.com/pnpm/pnpm/issues/6100).

The command runs `pnpm clean` followed by `pnpm install --frozen-lockfile`. Designed for CI/CD environments where reproducible builds are critical.

Aliases: `pnpm clean-install`, `pnpm ic`, `pnpm install-clean`
