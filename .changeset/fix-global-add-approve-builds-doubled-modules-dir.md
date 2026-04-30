---
"@pnpm/global.commands": patch
"pnpm": patch
---

Fix `ENOENT` symlink failure when `pnpm add -g` triggers the approve-builds prompt. The global add flow used to forward an absolute `modulesDir` (`<installDir>/node_modules`) into the install run by `approve-builds`. The install layer treats `modulesDir` as a path relative to `lockfileDir` and joins it again, producing a doubled path on Windows because `path.join` does not collapse an embedded absolute path. The hoist step then tried to `mkdir` and symlink under `<installDir>\<installDir>\node_modules\.pnpm\node_modules\...` and failed with `ENOENT` [#11403](https://github.com/pnpm/pnpm/issues/11403).
