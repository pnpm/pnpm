---
"@pnpm/pkg-manifest.utils": patch
"pnpm": patch
---

Fixed `pnpm add --save-exact`/`--save-prefix` and `pnpm update` writing a package's version with the `peerDependencies` range's prefix (e.g. `^19.2.7` instead of the requested `19.2.7`) whenever the same package also appeared in `peerDependencies`. `peerDependencies` is now merged in before `devDependencies`/`dependencies`/`optionalDependencies` when computing the current specifiers, so a real dependency entry always takes precedence over its own peer entry, matching how `getWantedDependencies` already treats peers elsewhere. See pnpm/pnpm#13108.
