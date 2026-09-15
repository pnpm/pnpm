---
"pacquet": patch
---

`pnpm add <git repository>` now names a repository that ships no `package.json` after its owner and repository, such as `@anthropics/skills`. Two repositories that share a name can be dependencies of the same project [pnpm/pnpm#14870](https://github.com/pnpm/pnpm/issues/14870).
