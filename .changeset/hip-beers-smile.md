---
"@pnpm/prepare-package": patch
"pnpm": patch
---

Only run prepublish scripts of git-hosted dependencies, if the dependency doesn't have a main file. In this case we can assume that the dependencies has to be built.
