---
"pacquet": patch
---

`pnpm add` no longer drops the other dependency groups from the install: adding a package with `optionalDependencies` no longer leaves dangling optional-dependency symlinks in the virtual store (`pnpm add -g @openai/codex` produced a `codex` bin that failed with "Missing optional dependency `@openai/codex-darwin-arm64`"), and a production `pnpm add` no longer removes the project's `devDependencies` from `pnpm-lock.yaml` and `node_modules`.
