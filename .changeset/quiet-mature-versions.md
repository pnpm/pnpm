---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

The `minimumReleaseAge` approval prompt now lists and counts each package version once. In pnpm v12, the version list is printed only once [pnpm/pnpm#15083](https://github.com/pnpm/pnpm/issues/15083).
