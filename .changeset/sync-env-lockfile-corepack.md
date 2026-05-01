---
"pnpm": patch
---

Fixed `packageManagerDependencies` going stale when pnpm is invoked through corepack. The lockfile sync (and the `devEngines.packageManager` version check) previously ran only when pnpm was invoked directly; under corepack the entire block was skipped, so a stale entry would persist even after the running pnpm version changed. The lockfile sync now runs regardless of how pnpm was invoked, while the pnpm-managed version switch (`onFail: 'download'`) remains skipped under corepack so it doesn't fight corepack's own version selection [#11397](https://github.com/pnpm/pnpm/issues/11397).
