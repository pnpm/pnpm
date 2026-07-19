## 1102.1.6

### Patch Changes

- Fixed `pnpm install` running out of memory while resolving large dependency graphs [#8441](https://github.com/pnpm/pnpm/issues/8441). The resolver kept full registry documents — per-version readmes, scripts, descriptions, and other install-irrelevant bulk — in memory for every package fetched with full metadata (optional dependencies, and packages re-fetched for `minimumReleaseAge`'s publish timestamps). Every retained document is now condensed down to the field set installation actually reads, which reduces peak resolution memory by several times on workspaces with more than a thousand packages.
