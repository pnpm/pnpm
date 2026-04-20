---
"@pnpm/store.pkg-finder": patch
"pnpm": patch
---

Fixed `pnpm sbom` and `pnpm licenses` failing to resolve license information for git-sourced dependencies (`git+https://`, `git+ssh://`, `github:` shorthand). These commands now correctly read the package manifest from the content-addressable store for `type: 'git'` resolutions [#11260](https://github.com/pnpm/pnpm/issues/11260).
