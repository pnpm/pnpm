---
"@pnpm/pnpr.client": minor
"@pnpm/installing.deps-installer": patch
"@pnpm/pnpr": patch
"pnpm": patch
"pacquet": patch
---

Installing through a `pnpr` server now links a workspace project at the directory its `publishConfig.directory` names, instead of linking the project root. An install that resolves through a server which does not forward the setting fails with `ERR_PNPM_PNPR_PUBLISH_DIRECTORY_MISMATCH` instead of writing a lockfile that points at the wrong directory, and the server rejects a `publishConfig.directory` that points outside its project [#14460](https://github.com/pnpm/pnpm/issues/14460).
