---
"pacquet": patch
---

A script listed in `syncInjectedDepsAfterScripts` now keeps a package's links to its own files as links in the injected copies, the way `pnpm install` writes them. Before, a directory link the script added became a real directory in the copies, or went missing with `deployAllFiles`.
