---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

Switching to the pnpm version a project pins in `packageManager` works again on hosts where that release ships no native binary, such as Alpine Linux with pnpm 10 or an Intel Mac with pnpm 11. The version switch verifies only the pnpm build it installs and runs, so a JavaScript pnpm no longer fails because the `@pnpm/exe` pinned beside it has no binary for the host [#13622](https://github.com/pnpm/pnpm/issues/13622).

When the build that would run is native and ships no binary for the host, pnpm now says so instead of reporting that the binary is missing from `pnpm-lock.yaml`.
