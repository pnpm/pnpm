---
"@pnpm/config.reader": patch
"pnpm": patch
---

Fix `cafile=<relative-path>` in `.npmrc` being read from the wrong directory when pnpm is invoked from a different cwd (e.g. `pnpm --dir <project> install` from a CI wrapper or monorepo script). The path is now resolved against the directory of the `.npmrc` that declared it, not `process.cwd()`. Before this fix the CA file silently failed to load — the install proceeded without the configured CA and the user only saw TLS errors against a private registry, with no log line tying back to the wrongly resolved path [#11624](https://github.com/pnpm/pnpm/issues/11624).
