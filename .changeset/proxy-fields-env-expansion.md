---
"@pnpm/config.reader": patch
"pnpm": patch
---

`${...}` environment-variable placeholders in the `httpProxy`, `httpsProxy`, `noProxy`, `proxy`, and `noproxy` settings are no longer expanded when these settings come from a project's `pnpm-workspace.yaml`. They now receive the same protection already applied to `registry`, `namedRegistries`, and `pnprServer`.
