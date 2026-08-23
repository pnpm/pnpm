---
"@pnpm/config.reader": patch
"@pnpm/workspace.commands": patch
"pacquet": patch
"pnpm": patch
---

The Rust CLI now honors five settings it recognized but ignored: `updateNotifier`, `legacyDirFiltering`, `initAuthorName` / `initAuthorEmail` / `initAuthorUrl`, `initLicense`, and `initVersion`. `pnpm install` and `pnpm add` check once a day for a newer pnpm and print how to get it (turn it off with `updateNotifier: false`); a `{<dir>}` filter selector can go back to matching the subtree below the directory with `legacyDirFiltering: true`; and `pnpm init` writes the configured author, license, and version into the `package.json` it scaffolds. `PNPM_CONFIG_INIT_VERSION` is now read as well.

`maxsockets`, npm's spelling of `maxSockets`, is no longer ignored: both spellings are read from `pnpm-workspace.yaml`, the global config file, and the environment.

A `lastUpdateCheck` timestamp dated in the future — after a clock change, a restored snapshot, or a hand-edited state file — no longer silences the update check until that time comes around.

`legacyDirFiltering` no longer reaches the `!{<workspace-root>}` selector a recursive `run` / `exec` / `add` / `test` generates for itself. Read as a subtree match, that selector named every project below the root, so a recursive command under the setting selected nothing at all [#14101](https://github.com/pnpm/pnpm/issues/14101).
