---
"pacquet": patch
---

Fixed `pnpm install` dropping a package that ships no `package.json` of its own from the lockfile. Such a package is now named after its alias and recorded at version `0.0.0` under `packages:` and `snapshots:`, and its extraction gets the placeholder `package.json` pnpm writes [#13410](https://github.com/pnpm/pnpm/issues/13410).
