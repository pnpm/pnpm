---
"pacquet": patch
---

A repeat `pnpm install` no longer reports "Already up to date" after `globalVirtualStoreDir` changes or `skipRuntimes` is turned off. It links the packages from the new directory, or installs the runtimes it skipped.
