---
"@pnpm/plugin-commands-patching": patch
---

After executing `pnpm patch-remove`, delete the corresponding dependent packages in the `.pnpm_patches` folder to prevent the originally deleted patches from interfering with the new patch content during subsequent operations.
