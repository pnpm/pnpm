---
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm pack` and `pnpm publish` no longer follow a symlinked workspace `LICENSE` file when injecting it into a package that has no license of its own. Following the symlink could pack bytes from outside the workspace into the published tarball.
