---
"pnpm": patch
---

Don't add the version of a local directory dependency to the lockfile. This information is not used anywhere by pnpm and is only causing more Git conflicts [#6695](https://github.com/pnpm/pnpm/pull/6695).
