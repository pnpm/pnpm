---
"@pnpm/config.reader": patch
"pnpm": patch
---

A project's `pnpm-workspace.yaml` can no longer decide where pnpm reads and writes state the project does not own. A repository that set `configDir` redirected the `auth.ini` that `pnpm login` writes the granted token to; `pnpmHomeDir` redirected the directory `pnpm setup` puts on the user's PATH; `bin` redirected where `pnpm install` links its dependencies' bins; and `packageManagerRegistries` redirected where pnpm downloads its own next version from. pnpm now resolves each of these before it reads the project manifest, and reports the ones a manifest tried to set [#13629](https://github.com/pnpm/pnpm/issues/13629).
