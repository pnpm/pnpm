---
"@pnpm/plugin-commands-publishing": patch
"@pnpm/plugin-commands-patching": patch
"@pnpm/directory-fetcher": patch
"pnpm": patch
---

`pnpm publish` should not pack the same file twice sometimes [#6997](https://github.com/pnpm/pnpm/issues/6997).

The fix was to update `npm-packlist` to the latest version.
