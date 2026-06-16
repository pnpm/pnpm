---
"@pnpm/building.after-install": patch
"pnpm": patch
---

Fixed `pnpm install` repeatedly prompting to remove and reinstall `node_modules` in a workspace package when `enableGlobalVirtualStore` is enabled. The post-install build step recorded a per-project `node_modules/.pnpm` virtual store directory in `node_modules/.modules.yaml`, overwriting the global `<storeDir>/links` value the install step had written. The next install then detected a virtual-store mismatch (`ERR_PNPM_UNEXPECTED_VIRTUAL_STORE`). The build step now derives the same global virtual store directory as the install step [#12307](https://github.com/pnpm/pnpm/issues/12307).
