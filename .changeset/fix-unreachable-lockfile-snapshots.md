---
"pacquet": patch
---

`pnpm install --frozen-lockfile` now prunes a `pnpm-lock.yaml` snapshot that no importer reaches, so a repeated install has nothing left to do. Such a snapshot was reimported on every run, which reran the lifecycle scripts and made `verifyDepsBeforeRun` install before every `pnpm run` and `pnpm exec` [#14891](https://github.com/pnpm/pnpm/issues/14891).
