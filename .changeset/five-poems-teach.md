---
"@pnpm/plugin-commands-patching": minor
"pnpm": minor
---

Change the default edit dir location when running `pnpm patch` from a temporary directory to `node_modules/.pnpm_patches/pkg[@version]` to allow the code editor to open the edit dir in the same file tree as the main project.
