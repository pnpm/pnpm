---
"pacquet": patch
---

`scriptShell` now selects the shell for lifecycle scripts too — dependency build scripts and a project's own `preinstall`/`install`/`postinstall`/`prepare` and `pnpm:devPreinstall` — not only for `pnpm run` and `pnpm exec`. A workspace that configures a shell was still getting the platform default (`sh` / `cmd`) for everything the install itself spawns.
