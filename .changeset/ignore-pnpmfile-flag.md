---
"pacquet": patch
---

`--ignore-pnpmfile` is accepted again, on every command pnpm takes it on: `install`, `add`, `update`, `dedupe`, `fetch`, `unlink`, `deploy`, `ci`, and `install-test` [#13808](https://github.com/pnpm/pnpm/issues/13808). The flag skips every pnpmfile hook the command would otherwise run: neither the workspace `.pnpmfile.cjs` nor the pnpmfiles of config dependencies are loaded, so no `readPackage`, `updateConfig`, `afterAllResolved`, custom resolver, or custom fetcher runs.
