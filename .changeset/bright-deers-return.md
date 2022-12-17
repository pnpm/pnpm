---
"@pnpm/plugin-commands-script-runners": minor
"@pnpm/types": minor
"pnpm": minor
---

New setting supported in the `package.json` that is in the root of the workspace: `pnpm.requiredScripts`. Scripts listed in this array will be required in each project of the worksapce. Otherwise, `pnpm -r run <script name>` will fail [#5569](https://github.com/pnpm/pnpm/issues/5569).
