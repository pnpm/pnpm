---
"@pnpm/config.reader": patch
"@pnpm/workspace.commands": patch
"@pnpm/workspace.projects-filter": patch
"pacquet": patch
"pnpm": patch
---

The Rust CLI now honors five settings it recognized but ignored: `updateNotifier`, `legacyDirFiltering`, `initAuthorName` / `initAuthorEmail` / `initAuthorUrl`, `initLicense`, and `initVersion`. `pnpm install` and `pnpm add` check once a day for a newer pnpm and print how to get it (turn it off with `updateNotifier: false`); a `{<dir>}` filter selector can go back to matching the subtree below the directory with `legacyDirFiltering: true`; and `pnpm init` writes the configured author, license, and version into the `package.json` it scaffolds. `PNPM_CONFIG_INIT_VERSION` is now read as well.

`maxsockets`, npm's spelling of `maxSockets`, is no longer ignored: both spellings are read from `pnpm-workspace.yaml`, the global config file, the environment, and the command line, in that increasing order of precedence — a value passed on the command line now wins even when the two sides spelled the setting differently.

A `lastUpdateCheck` timestamp dated in the future — after a clock change, a restored snapshot, or a hand-edited state file — no longer silences the update check until that time comes around.

`legacyDirFiltering` no longer reaches the workspace-root selectors pnpm generates for itself: the `!{<workspace-root>}` exclusion a recursive `run` / `exec` / `add` / `test` appends, and the `{<workspace-root>}` inclusion `--workspace-root` appends. Read as subtree matches they named every project below the root, so a recursive command under the setting selected nothing at all, and `--workspace-root` pulled in every project below the root instead of the root alone [#14101](https://github.com/pnpm/pnpm/issues/14101).
