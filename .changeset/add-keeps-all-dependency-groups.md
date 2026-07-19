---
"pacquet": patch
---

`pnpm add` no longer narrows the install to the dependency group it saves into. Previously the group leaked into the install as an include filter, so `pnpm add` of a package with `optionalDependencies` swept the just-installed optional packages from the virtual store, leaving dangling symlinks — `pnpm add -g @openai/codex` produced a broken `codex` bin that failed with "Missing optional dependency `@openai/codex-darwin-arm64`" — and a production `pnpm add` dropped the project's `devDependencies` from `pnpm-lock.yaml` and `node_modules`.
