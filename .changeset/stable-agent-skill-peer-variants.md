---
"pacquet": patch
---

Agent skill links now keep the same target across installs when a package version is installed with several sets of peer dependencies. An install no longer fails with `ERR_PNPM_AGENT_SKILL_PATH_OCCUPIED` on a skill link that pnpm created for another of those copies [pnpm/pnpm#17319](https://github.com/pnpm/pnpm/issues/17319).
