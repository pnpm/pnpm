---
"@pnpm/plugin-commands-publishing": patch
---

Do not modify the package.json file before packing the package. Do not copy LICENSE files from the root of the workspace (the files are still packed).
