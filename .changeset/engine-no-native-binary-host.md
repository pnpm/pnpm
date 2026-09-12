---
"@pnpm/engine.pm.commands": patch
"pnpm": patch
"pacquet": patch
---

The JavaScript pnpm can again switch to the pnpm version a project pins in `packageManager` on hosts where the native pnpm build ships no binary, such as Alpine Linux with pnpm 10 or an Intel Mac with pnpm 11 [#13622](https://github.com/pnpm/pnpm/issues/13622).

When the pnpm build being switched to is native and ships no binary for the host, pnpm now names the host target it lacks. pnpm reported that the binary was missing from `pnpm-lock.yaml`.
