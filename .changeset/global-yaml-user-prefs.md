---
"@pnpm/config.reader": patch
"pnpm": patch
---

Allow user-level preferences in the global `config.yaml`. The following settings can now be set in `~/.config/pnpm/config.yaml` (or via `pnpm config set --location global`) instead of being restricted to `pnpm-workspace.yaml`: `agent`, `globalVirtualStoreDir`, `initPackageManager`, `initType`, `registrySupportsTimeField`, `scriptShell`, `shellEmulator`, `sideEffectsCache`, `sideEffectsCacheReadonly`, `stateDir`, `strictDepBuilds`, `trustPolicy`, `trustPolicyExclude`, `trustPolicyIgnoreAfter`, `updateNotifier`, `useStderr`, `verifyDepsBeforeRun`, `verifyStoreIntegrity`, `virtualStoreDir`, `virtualStoreDirMaxLength` [#11474](https://github.com/pnpm/pnpm/issues/11474).
