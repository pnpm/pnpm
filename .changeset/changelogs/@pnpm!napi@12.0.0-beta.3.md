## 12.0.0-beta.3

### Minor Changes

- `readConfig` now returns `explicitSettings` — the camelCase names of settings the config cascade set explicitly — so hosts that layer the resolved config over their own defaults can forward only the values the user actually configured.

- Added a `returnListOfDepsRequiringBuild` install option. When it is set, `InstallResult.depsRequiringBuild` lists the dep path of every package whose files carry install scripts, whether or not the scripts were allowed to run, matching the TypeScript CLI's option of the same name. An install that computes no list, such as one served from the lockfile, leaves the field undefined.
