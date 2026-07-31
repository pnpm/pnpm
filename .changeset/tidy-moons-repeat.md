---
"@pnpm/napi": minor
---

Added a `returnListOfDepsRequiringBuild` install option. When it is set, `InstallResult.depsRequiringBuild` lists the dep path of every package whose files carry install scripts, whether or not the scripts were allowed to run, matching the TypeScript CLI's option of the same name. An install that computes no list, such as one served from the lockfile, leaves the field undefined.
