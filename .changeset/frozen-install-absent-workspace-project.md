---
"@pnpm/installing.deps-installer": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install --frozen-lockfile` again succeeds when a workspace project recorded in `pnpm-lock.yaml` has no directory, such as a project left out of a Docker build context. It still fails if the project's directory exists without a `package.json` [#16453](https://github.com/pnpm/pnpm/issues/16453).
