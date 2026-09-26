---
"pnpm": patch
"pacquet": patch
---

Do not run `prepare` lifecycle scripts when `pnpm install` is invoked with options that omit devDependencies (e.g., `pnpm install --prod`) or with package arguments.
